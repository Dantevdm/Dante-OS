//! What processor we are on, from CPUID.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use core::arch::x86_64::__cpuid;

/// (vendor, brand), e.g. ("AuthenticAMD", "QEMU Virtual CPU version 2.5+").
pub fn names() -> (String, String) {
    let r = __cpuid(0);
    let mut vendor = Vec::new();
    for v in [r.ebx, r.edx, r.ecx] {
        vendor.extend_from_slice(&v.to_le_bytes());
    }
    let mut brand = Vec::new();
    if __cpuid(0x8000_0000).eax >= 0x8000_0004 {
        for leaf in 0x8000_0002..=0x8000_0004 {
            let r = __cpuid(leaf);
            for v in [r.eax, r.ebx, r.ecx, r.edx] {
                brand.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    let text = |b: Vec<u8>| String::from_utf8_lossy(&b).trim_matches(|c: char| c == '\0' || c == ' ').to_string();
    (text(vendor), text(brand))
}
