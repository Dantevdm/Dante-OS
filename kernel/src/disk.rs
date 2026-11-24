//! The disk: a virtio block device, the standard virtual disk in QEMU and
//! most clouds. We use its simple "legacy" interface over I/O ports and wait
//! for each request to finish (no interrupts needed).

use core::sync::atomic::{Ordering, fence};
use spin::Mutex;
use x86_64::instructions::port::Port;

use crate::memory::{PAGE, dma_alloc};
use crate::pci;

pub const SECTOR: usize = 512;
const DATA_PAGES: u64 = 16;
const MAX_SECTORS: usize = (DATA_PAGES as usize * PAGE as usize) / SECTOR;

// Legacy virtio PCI registers (offsets from the I/O base).
const DEVICE_FEATURES: u16 = 0x00;
const GUEST_FEATURES: u16 = 0x04;
const QUEUE_ADDRESS: u16 = 0x08;
const QUEUE_SIZE: u16 = 0x0C;
const QUEUE_SELECT: u16 = 0x0E;
const QUEUE_NOTIFY: u16 = 0x10;
const STATUS: u16 = 0x12;
const CAPACITY: u16 = 0x14;

const DESC_NEXT: u16 = 1;
const DESC_WRITE: u16 = 2;

#[repr(C)]
struct Desc {
    addr: u64,
    len: u32,
    flags: u16,
    next: u16,
}

pub struct Disk {
    io: u16,
    size: u16,
    desc: *mut Desc,
    avail: *mut u16,
    used: *mut u16,
    avail_idx: u16,
    used_seen: u16,
    req_phys: u64,
    req: *mut u8,
    data_phys: u64,
    data: *mut u8,
    pub sectors: u64,
}

// Only touched while holding the DISK lock.
unsafe impl Send for Disk {}

pub static DISK: Mutex<Option<Disk>> = Mutex::new(None);

fn in8(p: u16) -> u8 {
    unsafe { Port::<u8>::new(p).read() }
}
fn out8(p: u16, v: u8) {
    unsafe { Port::<u8>::new(p).write(v) }
}
fn in16(p: u16) -> u16 {
    unsafe { Port::<u16>::new(p).read() }
}
fn out16(p: u16, v: u16) {
    unsafe { Port::<u16>::new(p).write(v) }
}
fn in32(p: u16) -> u32 {
    unsafe { Port::<u32>::new(p).read() }
}
fn out32(p: u16, v: u32) {
    unsafe { Port::<u32>::new(p).write(v) }
}

pub fn init() -> bool {
    // 0x1AF4:0x1001 is a virtio block device using the legacy interface.
    let Some(dev) = pci::find(0x1AF4, 0x1001) else {
        crate::log!("disk: none found");
        return false;
    };
    dev.enable();
    let io = (dev.bar(0) & !3) as u16;

    out8(io + STATUS, 0); // reset
    out8(io + STATUS, 1); // acknowledge
    out8(io + STATUS, 1 | 2); // driver
    let _features = in32(io + DEVICE_FEATURES);
    out32(io + GUEST_FEATURES, 0); // nothing optional

    out16(io + QUEUE_SELECT, 0);
    let size = in16(io + QUEUE_SIZE);
    if size == 0 {
        crate::log!("disk: device has no queue");
        return false;
    }
    let q = size as u64;
    let avail_off = 16 * q;
    let used_off = (avail_off + 6 + 2 * q).div_ceil(PAGE) * PAGE;
    let pages = (used_off + (6 + 8 * q).div_ceil(PAGE) * PAGE) / PAGE;
    let Some((qphys, qvirt)) = dma_alloc(pages) else { return false };
    let Some((req_phys, req)) = dma_alloc(1) else { return false };
    let Some((data_phys, data)) = dma_alloc(DATA_PAGES) else { return false };
    out32(io + QUEUE_ADDRESS, (qphys / PAGE) as u32);
    out8(io + STATUS, 1 | 2 | 4); // driver ready

    let sectors = in32(io + CAPACITY) as u64 | (in32(io + CAPACITY + 4) as u64) << 32;
    let disk = unsafe {
        Disk {
            io,
            size,
            desc: qvirt as *mut Desc,
            avail: qvirt.add(avail_off as usize) as *mut u16,
            used: qvirt.add(used_off as usize) as *mut u16,
            avail_idx: 0,
            used_seen: 0,
            req_phys,
            req,
            data_phys,
            data,
            sectors,
        }
    };
    // We poll for completion, so ask the device not to interrupt us.
    unsafe { disk.avail.write_volatile(1) };
    crate::log!("disk: virtio at {:02x}:{:02x}, {} MiB (queue {})", dev.bus, dev.slot, sectors * SECTOR as u64 / (1024 * 1024), size);
    *DISK.lock() = Some(disk);
    true
}

impl Disk {
    fn request(&mut self, write: bool, sector: u64, count: usize) -> bool {
        unsafe {
            // Header: type (0 read, 1 write), reserved, sector. Status byte after it.
            (self.req as *mut u32).write_volatile(write as u32);
            (self.req as *mut u32).add(1).write_volatile(0);
            (self.req as *mut u64).add(1).write_volatile(sector);
            self.req.add(16).write_volatile(0xFF);

            let d = self.desc;
            d.write_volatile(Desc { addr: self.req_phys, len: 16, flags: DESC_NEXT, next: 1 });
            let data_flags = DESC_NEXT | if write { 0 } else { DESC_WRITE };
            d.add(1).write_volatile(Desc { addr: self.data_phys, len: (count * SECTOR) as u32, flags: data_flags, next: 2 });
            d.add(2).write_volatile(Desc { addr: self.req_phys + 16, len: 1, flags: DESC_WRITE, next: 0 });

            let slot = (self.avail_idx % self.size) as usize;
            self.avail.add(2 + slot).write_volatile(0);
            fence(Ordering::SeqCst);
            self.avail_idx = self.avail_idx.wrapping_add(1);
            self.avail.add(1).write_volatile(self.avail_idx);
            fence(Ordering::SeqCst);
            out16(self.io + QUEUE_NOTIFY, 0);

            let mut spins = 0u64;
            while self.used.add(1).read_volatile() == self.used_seen {
                core::hint::spin_loop();
                spins += 1;
                if spins > 500_000_000 {
                    crate::log!("disk: request timed out");
                    return false;
                }
            }
            fence(Ordering::SeqCst);
            self.used_seen = self.used_seen.wrapping_add(1);
            let _ = in8(self.io + STATUS);
            self.req.add(16).read_volatile() == 0
        }
    }

    pub fn read(&mut self, sector: u64, buf: &mut [u8]) -> bool {
        for (i, chunk) in buf.chunks_mut(MAX_SECTORS * SECTOR).enumerate() {
            let count = chunk.len().div_ceil(SECTOR);
            if !self.request(false, sector + (i * MAX_SECTORS) as u64, count) {
                return false;
            }
            unsafe { core::ptr::copy_nonoverlapping(self.data, chunk.as_mut_ptr(), chunk.len()) };
        }
        true
    }

    pub fn write(&mut self, sector: u64, buf: &[u8]) -> bool {
        for (i, chunk) in buf.chunks(MAX_SECTORS * SECTOR).enumerate() {
            let count = chunk.len().div_ceil(SECTOR);
            unsafe {
                core::ptr::copy_nonoverlapping(chunk.as_ptr(), self.data, chunk.len());
                core::ptr::write_bytes(self.data.add(chunk.len()), 0, count * SECTOR - chunk.len());
            }
            if !self.request(true, sector + (i * MAX_SECTORS) as u64, count) {
                return false;
            }
        }
        true
    }
}
