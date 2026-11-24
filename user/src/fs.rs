//! Files and folders on the disk. Paths are relative to the folder the
//! program was started in.

use alloc::string::String;
use alloc::vec::Vec;

use crate::io;
use crate::sys::{self, Error};

/// An open file. Written data is saved when it's dropped (or the program ends).
pub struct File {
    fd: u64,
}

impl File {
    fn open_mode(path: &str, mode: u64) -> Result<File, Error> {
        let fd = sys::check(sys::call(sys::OPEN, path.as_ptr() as u64, path.len() as u64, mode, 0))?;
        Ok(File { fd })
    }

    /// Open a file for reading.
    pub fn open(path: &str) -> Result<File, Error> {
        Self::open_mode(path, 0)
    }

    /// Create a file, or empty an existing one, for writing.
    pub fn create(path: &str) -> Result<File, Error> {
        Self::open_mode(path, 1)
    }

    /// Open a file to add to its end, creating it if needed.
    pub fn append(path: &str) -> Result<File, Error> {
        Self::open_mode(path, 2)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize, Error> {
        io::read(self.fd, buf)
    }

    pub fn write(&mut self, bytes: &[u8]) -> Result<usize, Error> {
        io::write(self.fd, bytes)
    }

    /// Save and close, reporting any error (dropping saves too, but silently).
    pub fn close(self) -> Result<(), Error> {
        let fd = self.fd;
        core::mem::forget(self);
        sys::check(sys::call(sys::CLOSE, fd, 0, 0, 0)).map(|_| ())
    }
}

impl Drop for File {
    fn drop(&mut self) {
        sys::call(sys::CLOSE, self.fd, 0, 0, 0);
    }
}

pub fn read(path: &str) -> Result<Vec<u8>, Error> {
    let mut f = File::open(path)?;
    let mut data = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match f.read(&mut buf)? {
            0 => return Ok(data),
            n => data.extend_from_slice(&buf[..n]),
        }
    }
}

pub fn read_to_string(path: &str) -> Result<String, Error> {
    read(path).map(|d| String::from_utf8_lossy(&d).into_owned())
}

pub fn write(path: &str, data: &[u8]) -> Result<(), Error> {
    let mut f = File::create(path)?;
    f.write(data)?;
    f.close()
}

/// The names in a folder; folder names end in '/'.
pub fn list(path: &str) -> Result<Vec<String>, Error> {
    let mut buf = alloc::vec![0u8; 4096];
    loop {
        let n = sys::check(sys::call(sys::LIST, path.as_ptr() as u64, path.len() as u64, buf.as_mut_ptr() as u64, buf.len() as u64))? as usize;
        if n <= buf.len() {
            let text = String::from_utf8_lossy(&buf[..n]);
            return Ok(text.lines().map(String::from).collect());
        }
        buf.resize(n, 0);
    }
}

pub fn remove(path: &str) -> Result<(), Error> {
    sys::check(sys::call(sys::REMOVE, path.as_ptr() as u64, path.len() as u64, 0, 0)).map(|_| ())
}

pub fn mkdir(path: &str) -> Result<(), Error> {
    sys::check(sys::call(sys::MKDIR, path.as_ptr() as u64, path.len() as u64, 0, 0)).map(|_| ())
}
