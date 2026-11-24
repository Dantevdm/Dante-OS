//! The programs that come with Dante. They are built from ../user, carried
//! inside the kernel, and copied to /Programs on the disk at startup.

use alloc::format;

include!(concat!(env!("OUT_DIR"), "/programs.rs"));

pub const DIR: &str = "/Programs";

pub fn install() {
    if crate::fs::stat(DIR).is_err() && crate::fs::mkdir(DIR).is_err() {
        crate::log!("programs: could not create {DIR}");
        return;
    }
    for (name, bytes) in PROGRAMS {
        let path = format!("{DIR}/{name}");
        let same = crate::fs::read(&path).is_ok_and(|d| d == *bytes);
        if !same {
            if let Err(e) = crate::fs::write(&path, bytes) {
                crate::log!("programs: could not write {path}: {}", e.text());
            }
        }
    }
    crate::log!("programs: {} in {DIR}", PROGRAMS.len());
}
