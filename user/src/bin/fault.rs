//! Reads memory it doesn't own. The kernel stops the program, not the computer.

#![no_std]
#![no_main]

dante::main!(main);

fn main(_args: &str) {
    let value = unsafe { core::ptr::read_volatile(0x10 as *const u64) };
    dante::println!("read {value}");
}
