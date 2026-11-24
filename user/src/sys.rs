//! System calls. The kernel's side is kernel/src/process.rs.

use core::arch::asm;
use core::fmt;

pub const EXIT: u64 = 0;
pub const WRITE: u64 = 1;
pub const SLEEP: u64 = 2;
pub const MILLIS: u64 = 3;
pub const YIELD: u64 = 4;
pub const READ: u64 = 5;
pub const OPEN: u64 = 6;
pub const CLOSE: u64 = 7;
pub const LIST: u64 = 8;
pub const GROW: u64 = 9;
pub const REMOVE: u64 = 10;
pub const MKDIR: u64 = 11;

#[inline(always)]
pub fn call(n: u64, a: u64, b: u64, c: u64, d: u64) -> u64 {
    let ret;
    unsafe {
        asm!("int 0x80", inlateout("rax") n => ret, in("rdi") a, in("rsi") b, in("rdx") c, in("r10") d, options(nostack));
    }
    ret
}

/// What went wrong in a system call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Error {
    Invalid,
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    NoSpace,
    TooBig,
    BadName,
    Io,
    NoMemory,
    TooMany,
}

impl Error {
    pub fn text(self) -> &'static str {
        match self {
            Error::Invalid => "invalid argument",
            Error::NotFound => "no such file or directory",
            Error::Exists => "already exists",
            Error::NotDir => "not a directory",
            Error::IsDir => "is a directory",
            Error::NotEmpty => "directory not empty",
            Error::NoSpace => "disk full",
            Error::TooBig => "file too large",
            Error::BadName => "invalid name",
            Error::Io => "disk error",
            Error::NoMemory => "out of memory",
            Error::TooMany => "too many open files",
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(self.text())
    }
}

/// Turn a raw result into a value or an error.
pub fn check(ret: u64) -> Result<u64, Error> {
    let code = ret as i64;
    if !(-4095..0).contains(&code) {
        return Ok(ret);
    }
    Err(match -code {
        2 => Error::NotFound,
        3 => Error::Exists,
        4 => Error::NotDir,
        5 => Error::IsDir,
        6 => Error::NotEmpty,
        7 => Error::NoSpace,
        8 => Error::TooBig,
        9 => Error::BadName,
        10 => Error::Io,
        11 => Error::NoMemory,
        12 => Error::TooMany,
        _ => Error::Invalid,
    })
}
