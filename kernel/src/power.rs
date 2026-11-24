//! Restarting and powering off.

use x86_64::instructions::port::Port;

pub fn reboot() -> ! {
    crate::log!("power: restarting");
    unsafe { Port::<u8>::new(0x64).write(0xFE) };
    crate::halt();
}

/// Powers off under QEMU or Bochs. Real ACPI shutdown comes later; on other
/// machines this returns and the caller says so.
pub fn shutdown() {
    crate::log!("power: shutting down");
    unsafe {
        Port::<u16>::new(0x604).write(0x2000);
        Port::<u16>::new(0xB004).write(0x2000);
    }
}
