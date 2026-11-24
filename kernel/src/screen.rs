//! Framebuffer drawing: gradients, anti-aliased lines and circles, and text.
//!
//! Everything blends with what is already on screen, so shapes and text sit
//! cleanly on top of the First Light gradient.

use core::fmt;
use limine::framebuffer::Framebuffer;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

pub use crate::gfx::{Gradient, Rgb, Surface};

pub struct Screen {
    base: *mut u8,
    pub width: usize,
    pub height: usize,
    pitch: usize,
    bytes_per_pixel: usize,
    shifts: (u8, u8, u8),
    pub background: Gradient,
}

// The framebuffer is only touched from one core for now.
unsafe impl Send for Screen {}

impl Screen {
    pub fn new(fb: &Framebuffer, background: Gradient) -> Self {
        Self {
            base: fb.address() as *mut u8,
            width: fb.width as usize,
            height: fb.height as usize,
            pitch: fb.pitch as usize,
            bytes_per_pixel: (fb.bpp as usize).div_ceil(8),
            shifts: (fb.red_mask_shift, fb.green_mask_shift, fb.blue_mask_shift),
            background,
        }
    }

    fn offset(&self, x: usize, y: usize) -> usize {
        y * self.pitch + x * self.bytes_per_pixel
    }

    pub fn put(&mut self, x: usize, y: usize, c: Rgb) {
        if x >= self.width || y >= self.height {
            return;
        }
        let px = (c.0 as u32) << self.shifts.0 | (c.1 as u32) << self.shifts.1 | (c.2 as u32) << self.shifts.2;
        unsafe {
            let p = self.base.add(self.offset(x, y));
            if self.bytes_per_pixel == 4 {
                (p as *mut u32).write_volatile(px);
            } else {
                for i in 0..self.bytes_per_pixel {
                    p.add(i).write_volatile((px >> (i * 8)) as u8);
                }
            }
        }
    }

    pub fn get(&self, x: usize, y: usize) -> Rgb {
        if x >= self.width || y >= self.height {
            return Rgb(0, 0, 0);
        }
        let px = unsafe {
            let p = self.base.add(self.offset(x, y));
            if self.bytes_per_pixel == 4 {
                (p as *const u32).read_volatile()
            } else {
                let mut v = 0u32;
                for i in 0..self.bytes_per_pixel {
                    v |= (p.add(i).read_volatile() as u32) << (i * 8);
                }
                v
            }
        };
        Rgb((px >> self.shifts.0) as u8, (px >> self.shifts.1) as u8, (px >> self.shifts.2) as u8)
    }

    // ---- Background ----

    pub fn background_at(&self, y: usize) -> Rgb {
        self.background.at(y as f32 / self.height.max(1) as f32)
    }

    /// Restore the background gradient inside a rectangle.
    pub fn clear_rect(&mut self, x: usize, y: usize, w: usize, h: usize) {
        for yy in y..(y + h).min(self.height) {
            let c = self.background_at(yy);
            for xx in x..(x + w).min(self.width) {
                self.put(xx, yy, c);
            }
        }
    }

    pub fn clear(&mut self) {
        self.clear_rect(0, 0, self.width, self.height);
    }

    pub fn glyph_width(size: RasterHeight, weight: FontWeight) -> usize {
        crate::gfx::glyph_width(size, weight)
    }

    pub fn text_width(s: &str, size: RasterHeight, weight: FontWeight, spacing: usize) -> usize {
        crate::gfx::text_width(s, size, weight, spacing)
    }

    /// Copy a row of 0xAARRGGBB pixels to the screen (the alpha is ignored).
    pub fn write_row(&mut self, x: usize, y: usize, row: &[u32]) {
        if y >= self.height || x >= self.width {
            return;
        }
        let n = row.len().min(self.width - x);
        let fast = self.bytes_per_pixel == 4 && self.shifts == (16, 8, 0);
        unsafe {
            let p = self.base.add(self.offset(x, y));
            if fast {
                core::ptr::copy_nonoverlapping(row.as_ptr(), p as *mut u32, n);
            } else {
                for (i, &v) in row[..n].iter().enumerate() {
                    let c = Rgb::from_u32(v);
                    let px = (c.0 as u32) << self.shifts.0 | (c.1 as u32) << self.shifts.1 | (c.2 as u32) << self.shifts.2;
                    let q = p.add(i * self.bytes_per_pixel);
                    for b in 0..self.bytes_per_pixel {
                        q.add(b).write_volatile((px >> (b * 8)) as u8);
                    }
                }
            }
        }
    }
}

impl Surface for Screen {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }
    fn put(&mut self, x: usize, y: usize, c: Rgb) {
        Screen::put(self, x, y, c)
    }
    fn get(&self, x: usize, y: usize) -> Rgb {
        Screen::get(self, x, y)
    }
}

/// Formats into a fixed buffer so we can print numbers without a heap.
pub struct Buf<const N: usize> {
    data: [u8; N],
    len: usize,
}

impl<const N: usize> Buf<N> {
    pub const fn new() -> Self {
        Self { data: [0; N], len: 0 }
    }
    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.data[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> fmt::Write for Buf<N> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let bytes = s.as_bytes();
        let n = bytes.len().min(N - self.len);
        self.data[self.len..self.len + n].copy_from_slice(&bytes[..n]);
        self.len += n;
        Ok(())
    }
}
