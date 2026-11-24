//! Framebuffer drawing: gradients, anti-aliased lines and circles, and text.
//!
//! Everything blends with what is already on screen, so shapes and text sit
//! cleanly on top of the First Light gradient.

use core::fmt;
use limine::framebuffer::Framebuffer;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster};

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub const fn hex(v: u32) -> Self {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Mix `self` towards `other` by `t` (0.0 = self, 1.0 = other).
    pub fn mix(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t + 0.5) as u8;
        Rgb(m(self.0, other.0), m(self.1, other.1), m(self.2, other.2))
    }
}

/// A vertical gradient with three stops: top, `mid` (at `mid_at` of the height) and bottom.
#[derive(Clone, Copy)]
pub struct Gradient {
    pub top: Rgb,
    pub mid: Rgb,
    pub mid_at: f32,
    pub bottom: Rgb,
}

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

    /// Paint `c` over the pixel with coverage `alpha` (0.0 to 1.0).
    pub fn blend(&mut self, x: i64, y: i64, c: Rgb, alpha: f32) {
        if x < 0 || y < 0 || alpha <= 0.0 {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if x >= self.width || y >= self.height {
            return;
        }
        if alpha >= 1.0 {
            self.put(x, y, c);
        } else {
            let under = self.get(x, y);
            self.put(x, y, under.mix(c, alpha));
        }
    }

    pub fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: Rgb) {
        for yy in y..(y + h).min(self.height) {
            for xx in x..(x + w).min(self.width) {
                self.put(xx, yy, c);
            }
        }
    }

    // ---- Background ----

    pub fn background_at(&self, y: usize) -> Rgb {
        let g = self.background;
        let t = y as f32 / self.height.max(1) as f32;
        if t < g.mid_at {
            g.top.mix(g.mid, t / g.mid_at)
        } else {
            g.mid.mix(g.bottom, (t - g.mid_at) / (1.0 - g.mid_at))
        }
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

    // ---- Anti-aliased shapes ----

    /// A smooth line (Xiaolin Wu's algorithm).
    pub fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb, alpha: f32) {
        let (mut x0, mut y0, mut x1, mut y1) = (x0, y0, x1, y1);
        let steep = (y1 - y0).abs() > (x1 - x0).abs();
        if steep {
            core::mem::swap(&mut x0, &mut y0);
            core::mem::swap(&mut x1, &mut y1);
        }
        if x0 > x1 {
            core::mem::swap(&mut x0, &mut x1);
            core::mem::swap(&mut y0, &mut y1);
        }
        let dx = x1 - x0;
        let grad = if dx == 0.0 { 1.0 } else { (y1 - y0) / dx };
        let fpart = |v: f32| v - libm::floorf(v);
        let rfpart = |v: f32| 1.0 - fpart(v);
        let plot = |s: &mut Self, x: f32, y: f32, a: f32| {
            let (px, py) = if steep { (y, x) } else { (x, y) };
            s.blend(px as i64, py as i64, c, a * alpha);
        };

        let xend = libm::roundf(x0);
        let yend = y0 + grad * (xend - x0);
        let xgap = rfpart(x0 + 0.5);
        let ypxl1 = libm::floorf(yend);
        plot(self, xend, ypxl1, rfpart(yend) * xgap);
        plot(self, xend, ypxl1 + 1.0, fpart(yend) * xgap);
        let xpxl1 = xend;
        let mut intery = yend + grad;

        let xend = libm::roundf(x1);
        let yend = y1 + grad * (xend - x1);
        let xgap = fpart(x1 + 0.5);
        let ypxl2 = libm::floorf(yend);
        plot(self, xend, ypxl2, rfpart(yend) * xgap);
        plot(self, xend, ypxl2 + 1.0, fpart(yend) * xgap);
        let xpxl2 = xend;

        let mut x = xpxl1 + 1.0;
        while x < xpxl2 {
            let fy = libm::floorf(intery);
            plot(self, x, fy, rfpart(intery));
            plot(self, x, fy + 1.0, fpart(intery));
            intery += grad;
            x += 1.0;
        }
    }

    /// A slightly heavier smooth line, used for drawing outlines.
    pub fn stroke(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        // Two Wu lines a third of a pixel apart read as a ~1.4 px stroke.
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = libm::sqrtf(dx * dx + dy * dy).max(0.001);
        let (nx, ny) = (-dy / len * 0.33, dx / len * 0.33);
        self.line(x0 + nx, y0 + ny, x1 + nx, y1 + ny, c, 0.75);
        self.line(x0 - nx, y0 - ny, x1 - nx, y1 - ny, c, 0.75);
    }

    /// A smooth circle outline.
    pub fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Rgb) {
        let reach = (r + width + 2.0) as i64;
        for y in -reach..=reach {
            for x in -reach..=reach {
                let (px, py) = (cx as i64 + x, cy as i64 + y);
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let d = libm::sqrtf(dx * dx + dy * dy);
                let a = (width / 2.0 + 0.5 - (d - r).abs()).clamp(0.0, 1.0);
                self.blend(px, py, c, a);
            }
        }
    }

    /// A smooth filled circle.
    pub fn disc(&mut self, cx: f32, cy: f32, r: f32, c: Rgb) {
        let reach = (r + 2.0) as i64;
        for y in -reach..=reach {
            for x in -reach..=reach {
                let (px, py) = (cx as i64 + x, cy as i64 + y);
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let d = libm::sqrtf(dx * dx + dy * dy);
                self.blend(px, py, c, (r + 0.5 - d).clamp(0.0, 1.0));
            }
        }
    }

    /// A rounded rectangle, filled with `fill` at `alpha` and outlined with `border`.
    /// Edges are anti-aliased.
    pub fn rounded_rect(&mut self, x: i64, y: i64, w: i64, h: i64, r: f32, fill: Rgb, alpha: f32, border: Option<Rgb>) {
        let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
        let (cx, cy) = (x as f32 + hw, y as f32 + hh);
        for py in y - 1..y + h + 1 {
            for px in x - 1..x + w + 1 {
                let qx = (px as f32 + 0.5 - cx).abs() - (hw - r);
                let qy = (py as f32 + 0.5 - cy).abs() - (hh - r);
                let outside = if qx > 0.0 && qy > 0.0 { libm::sqrtf(qx * qx + qy * qy) } else { qx.max(qy).max(0.0) };
                let d = outside + qx.max(qy).min(0.0) - r; // signed distance to the edge, negative inside
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover <= 0.0 {
                    continue;
                }
                self.blend(px, py, fill, alpha * cover);
                if let Some(b) = border {
                    let edge = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
                    self.blend(px, py, b, edge);
                }
            }
        }
    }

    // ---- Text ----

    pub fn glyph_width(size: RasterHeight, weight: FontWeight) -> usize {
        noto_sans_mono_bitmap::get_raster_width(weight, size)
    }

    pub fn text_width(s: &str, size: RasterHeight, weight: FontWeight, spacing: usize) -> usize {
        let n = s.chars().count();
        if n == 0 { 0 } else { n * (Self::glyph_width(size, weight) + spacing) - spacing }
    }

    /// Draws text blended over what is underneath. Returns the x after the last glyph.
    pub fn text(&mut self, x: usize, y: usize, s: &str, size: RasterHeight, weight: FontWeight, fg: Rgb, spacing: usize, opacity: f32) -> usize {
        let mut cx = x;
        for ch in s.chars() {
            let glyph = get_raster(ch, weight, size).or_else(|| get_raster('?', weight, size));
            let Some(glyph) = glyph else { continue };
            for (gy, row) in glyph.rows().enumerate() {
                for (gx, &a) in row.iter().enumerate() {
                    if a > 0 {
                        self.blend((cx + gx) as i64, (y + gy) as i64, fg, a as f32 / 255.0 * opacity);
                    }
                }
            }
            cx += glyph.width() + spacing;
        }
        cx
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
