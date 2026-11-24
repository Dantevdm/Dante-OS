//! The terminal: printing and typed input.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt;

use crate::sys;

pub fn write(fd: u64, bytes: &[u8]) -> Result<usize, sys::Error> {
    sys::check(sys::call(sys::WRITE, fd, bytes.as_ptr() as u64, bytes.len() as u64, 0)).map(|n| n as usize)
}

pub fn read(fd: u64, buf: &mut [u8]) -> Result<usize, sys::Error> {
    sys::check(sys::call(sys::READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64, 0)).map(|n| n as usize)
}

pub struct Stdout;

impl fmt::Write for Stdout {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        write(1, s.as_bytes()).map(|_| ()).map_err(|_| fmt::Error)
    }
}

/// The next line typed into the terminal, without its newline.
/// `None` once input has ended (Ctrl+D).
pub fn read_line() -> Option<String> {
    let mut line = Vec::new();
    let mut buf = [0u8; 256];
    loop {
        let n = read(0, &mut buf).unwrap_or(0);
        if n == 0 {
            return if line.is_empty() { None } else { Some(String::from_utf8_lossy(&line).into_owned()) };
        }
        line.extend_from_slice(&buf[..n]);
        if line.last() == Some(&b'\n') {
            line.pop();
            return Some(String::from_utf8_lossy(&line).into_owned());
        }
    }
}

/// Everything typed until input ends.
pub fn read_all() -> Vec<u8> {
    let mut all = Vec::new();
    let mut buf = [0u8; 256];
    while let Ok(n @ 1..) = read(0, &mut buf) {
        all.extend_from_slice(&buf[..n]);
    }
    all
}
