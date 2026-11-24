//! The Dante runtime for user programs: system calls, printing, typed input,
//! files, memory, and the entry point that calls the program's `main`.
//!
//! A program looks like this:
//!
//! ```ignore
//! #![no_std]
//! #![no_main]
//! dante::main!(main);
//! fn main(args: &str) {
//!     dante::println!("hello");
//! }
//! ```
//!
//! `alloc` is available, so programs can use `Vec`, `String` and friends.

#![no_std]

extern crate alloc;

pub mod fs;
pub mod io;
mod heap;
mod sys;

pub use alloc::{format, string::String, vec, vec::Vec};
pub use sys::Error;

/// End the program.
pub fn exit(code: i64) -> ! {
    sys::call(sys::EXIT, code as u64, 0, 0, 0);
    unreachable!()
}

/// Pause for `ms` milliseconds, letting other programs run.
pub fn sleep(ms: u64) {
    sys::call(sys::SLEEP, ms, 0, 0, 0);
}

/// Milliseconds since the computer started.
pub fn millis() -> u64 {
    sys::call(sys::MILLIS, 0, 0, 0, 0)
}

/// Give the rest of this time slice to other programs.
pub fn yield_now() {
    sys::call(sys::YIELD, 0, 0, 0, 0);
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {{
        let _ = core::fmt::Write::write_fmt(&mut $crate::io::Stdout, format_args!($($arg)*));
    }};
}

#[macro_export]
macro_rules! println {
    () => { $crate::print!("\n") };
    ($($arg:tt)*) => { $crate::print!("{}\n", format_args!($($arg)*)) };
}

/// Names the program's main function: `fn main(args: &str)`.
#[macro_export]
macro_rules! main {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub fn __dante_main(args: &str) {
            $f(args)
        }
    };
}

unsafe extern "Rust" {
    fn __dante_main(args: &str);
}

/// Where the kernel starts the program: `rdi` and `rsi` hold the arguments.
#[unsafe(no_mangle)]
extern "C" fn _start(args: *const u8, len: usize) -> ! {
    let args = unsafe { core::str::from_utf8_unchecked(core::slice::from_raw_parts(args, len)) };
    unsafe { __dante_main(args) };
    exit(0)
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("panic: {}", info.message());
    exit(101)
}
