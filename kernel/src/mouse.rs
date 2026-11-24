//! Mouse input.
//!
//! Two sources:
//! - the PS/2 mouse, which reports relative movement (real hardware);
//! - the VMware "backdoor" absolute pointer, which QEMU and VMware provide. It
//!   reports where the host's pointer is, so the cursor follows the Mac's mouse
//!   without the window having to grab it.
//!
//! The interrupt handler only queues events; the window manager reads them.

use core::arch::asm;
use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use x86_64::instructions::port::Port;

#[derive(Clone, Copy)]
pub enum Event {
    /// Position in 0..=0xFFFF on both axes.
    Abs { x: u16, y: u16, buttons: u8 },
    Rel { dx: i16, dy: i16, buttons: u8 },
}

pub const LEFT: u8 = 1;

const QUEUE: usize = 256;
static mut EVENTS: [Event; QUEUE] = [Event::Rel { dx: 0, dy: 0, buttons: 0 }; QUEUE];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);
static ABSOLUTE: AtomicBool = AtomicBool::new(false);

fn push(e: Event) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % QUEUE;
    if next == TAIL.load(Ordering::Acquire) {
        return;
    }
    unsafe { (&raw mut EVENTS).cast::<Event>().add(head).write_volatile(e) };
    HEAD.store(next, Ordering::Release);
}

pub fn next_event() -> Option<Event> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let e = unsafe { (&raw const EVENTS).cast::<Event>().add(tail).read_volatile() };
    TAIL.store((tail + 1) % QUEUE, Ordering::Release);
    Some(e)
}

// ---- PS/2 controller ----

fn status() -> u8 {
    unsafe { Port::<u8>::new(0x64).read() }
}

fn wait_write() {
    for _ in 0..100_000 {
        if status() & 2 == 0 {
            return;
        }
    }
}

fn read_data() -> Option<u8> {
    for _ in 0..100_000 {
        if status() & 1 != 0 {
            return Some(unsafe { Port::<u8>::new(0x60).read() });
        }
    }
    None
}

fn command(c: u8) {
    wait_write();
    unsafe { Port::<u8>::new(0x64).write(c) };
}

fn write_data(b: u8) {
    wait_write();
    unsafe { Port::<u8>::new(0x60).write(b) };
}

fn mouse_send(b: u8) -> Option<u8> {
    command(0xD4);
    write_data(b);
    read_data()
}

/// Turn on the PS/2 mouse (and the absolute pointer, if there is one).
/// Call before interrupts are enabled.
pub fn init() {
    while status() & 1 != 0 {
        unsafe { Port::<u8>::new(0x60).read() };
    }
    command(0xA8); // enable the second (mouse) port
    command(0x20);
    let config = read_data().unwrap_or(0x47);
    command(0x60);
    write_data((config | 0x02) & !0x20); // mouse interrupts on, mouse clock on
    let defaults = mouse_send(0xF6);
    let stream = mouse_send(0xF4);
    crate::log!("mouse: PS/2 ready (ack {:?} {:?})", defaults, stream);

    if vm_enable() {
        ABSOLUTE.store(true, Ordering::Relaxed);
        crate::log!("mouse: absolute pointer on (VMware backdoor)");
    }
}

// ---- Interrupt ----

static mut PACKET: [u8; 3] = [0; 3];
static mut INDEX: usize = 0;

/// Called from the mouse interrupt (IRQ 12).
pub fn on_irq() {
    let st = status();
    if st & 0x21 != 0x21 {
        return; // nothing from the mouse port
    }
    let byte = unsafe { Port::<u8>::new(0x60).read() };
    if ABSOLUTE.load(Ordering::Relaxed) {
        vm_poll();
        return;
    }
    // Only the interrupt touches the packet state.
    unsafe {
        let (packet, index) = (&mut *(&raw mut PACKET), &mut *(&raw mut INDEX));
        if *index == 0 && byte & 0x08 == 0 {
            return; // out of sync: the first byte always has bit 3 set
        }
        packet[*index] = byte;
        *index += 1;
        if *index < 3 {
            return;
        }
        *index = 0;
        let flags = packet[0];
        if flags & 0xC0 != 0 {
            return; // overflow
        }
        let dx = packet[1] as i16 - (((flags as i16) << 4) & 0x100);
        let dy = packet[2] as i16 - (((flags as i16) << 3) & 0x100);
        push(Event::Rel { dx, dy: -dy, buttons: flags & 7 });
    }
}

// ---- VMware backdoor absolute pointer ----

const MAGIC: u32 = 0x564D_5868;
const PORT: u32 = 0x5658;
const GETVERSION: u32 = 10;
const ABSPOINTER_DATA: u32 = 39;
const ABSPOINTER_STATUS: u32 = 40;
const ABSPOINTER_COMMAND: u32 = 41;
const CMD_ENABLE: u32 = 0x4541_4552;
const CMD_REQUEST_ABSOLUTE: u32 = 0x5342_4152;
const VERSION_ID: u32 = 0x3442_554A;

fn backdoor(cmd: u32, arg: u32) -> (u32, u32, u32, u32) {
    let (a, b, c, d): (u32, u64, u32, u32);
    unsafe {
        // rbx is reserved by the compiler, so swap our value in and out.
        asm!(
            "xchg rbx, {b}",
            "in eax, dx",
            "xchg rbx, {b}",
            b = inout(reg) arg as u64 => b,
            inout("eax") MAGIC => a,
            inout("ecx") cmd => c,
            inout("edx") PORT => d,
            options(nostack),
        );
    }
    (a, b as u32, c, d)
}

fn vm_enable() -> bool {
    if backdoor(GETVERSION, 0).1 != MAGIC {
        return false;
    }
    backdoor(ABSPOINTER_COMMAND, CMD_ENABLE);
    let mut queued = 0;
    for _ in 0..1000 {
        queued = backdoor(ABSPOINTER_STATUS, 0).0;
        if queued & 0xFFFF != 0 {
            break;
        }
    }
    if queued & 0xFFFF == 0 || queued & 0xFFFF_0000 == 0xFFFF_0000 {
        return false;
    }
    if backdoor(ABSPOINTER_DATA, 1).0 != VERSION_ID {
        return false;
    }
    backdoor(ABSPOINTER_COMMAND, CMD_REQUEST_ABSOLUTE);
    true
}

fn vm_poll() {
    loop {
        let st = backdoor(ABSPOINTER_STATUS, 0).0;
        if st & 0xFFFF_0000 == 0xFFFF_0000 {
            return;
        }
        if st & 0xFFFF < 4 {
            return;
        }
        let (flags, x, y, _wheel) = backdoor(ABSPOINTER_DATA, 4);
        let mut buttons = 0;
        if flags & 0x20 != 0 {
            buttons |= 1;
        }
        if flags & 0x10 != 0 {
            buttons |= 2;
        }
        if flags & 0x08 != 0 {
            buttons |= 4;
        }
        push(Event::Abs { x: x as u16, y: y as u16, buttons });
    }
}
