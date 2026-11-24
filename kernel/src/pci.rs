//! PCI: finding devices through the legacy configuration ports.

use x86_64::instructions::port::Port;

#[derive(Clone, Copy, Debug)]
pub struct Device {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor: u16,
    pub device: u16,
}

fn address(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    0x8000_0000 | (bus as u32) << 16 | (slot as u32) << 11 | (func as u32) << 8 | (offset as u32 & 0xFC)
}

pub fn read32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    unsafe {
        Port::<u32>::new(0xCF8).write(address(bus, slot, func, offset));
        Port::<u32>::new(0xCFC).read()
    }
}

pub fn write32(bus: u8, slot: u8, func: u8, offset: u8, value: u32) {
    unsafe {
        Port::<u32>::new(0xCF8).write(address(bus, slot, func, offset));
        Port::<u32>::new(0xCFC).write(value);
    }
}

impl Device {
    pub fn read(&self, offset: u8) -> u32 {
        read32(self.bus, self.slot, self.func, offset)
    }

    pub fn bar(&self, n: u8) -> u32 {
        self.read(0x10 + 4 * n)
    }

    /// Let the device respond to I/O ports and read/write memory itself.
    pub fn enable(&self) {
        let cmd = self.read(0x04);
        write32(self.bus, self.slot, self.func, 0x04, (cmd & 0xFFFF_0000) | (cmd & 0xFFFF) | 0b101);
    }
}

/// Find the first device with this vendor and device id.
pub fn find(vendor: u16, device: u16) -> Option<Device> {
    for bus in 0..=255u8 {
        for slot in 0..32u8 {
            for func in 0..8u8 {
                let id = read32(bus, slot, func, 0);
                if id & 0xFFFF == 0xFFFF {
                    if func == 0 {
                        break;
                    }
                    continue;
                }
                let d = Device { bus, slot, func, vendor: id as u16, device: (id >> 16) as u16 };
                if d.vendor == vendor && d.device == device {
                    return Some(d);
                }
                // Single-function devices only answer on function 0.
                if func == 0 && (read32(bus, slot, 0, 0x0C) >> 16) & 0x80 == 0 {
                    break;
                }
            }
        }
    }
    None
}
