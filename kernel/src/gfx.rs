//! Drawing: colours, rectangles, and a `Surface` trait that gives anything with
//! pixels (the screen itself, or an off-screen `Canvas`) the same smooth shapes
//! and text.

use alloc::vec;
use alloc::vec::Vec;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight, get_raster, get_raster_width};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
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

    /// Packed as 0xAARRGGBB.
    pub const fn argb(self, a: u8) -> u32 {
        (a as u32) << 24 | (self.0 as u32) << 16 | (self.1 as u32) << 8 | self.2 as u32
    }

    pub const fn from_u32(v: u32) -> Self {
        Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
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

impl Gradient {
    pub fn at(&self, t: f32) -> Rgb {
        if t < self.mid_at {
            self.top.mix(self.mid, t / self.mid_at)
        } else {
            self.mid.mix(self.bottom, (t - self.mid_at) / (1.0 - self.mid_at))
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }
    pub fn right(&self) -> i32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.h
    }
    pub fn is_empty(&self) -> bool {
        self.w <= 0 || self.h <= 0
    }
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && y >= self.y && x < self.right() && y < self.bottom()
    }
    pub fn intersect(&self, o: Rect) -> Rect {
        let (x0, y0) = (self.x.max(o.x), self.y.max(o.y));
        let (x1, y1) = (self.right().min(o.right()), self.bottom().min(o.bottom()));
        if x1 <= x0 || y1 <= y0 { Rect::default() } else { Rect::new(x0, y0, x1 - x0, y1 - y0) }
    }
    pub fn intersects(&self, o: Rect) -> bool {
        !self.intersect(o).is_empty()
    }
    pub fn union(&self, o: Rect) -> Rect {
        if self.is_empty() {
            return o;
        }
        if o.is_empty() {
            return *self;
        }
        let (x0, y0) = (self.x.min(o.x), self.y.min(o.y));
        let (x1, y1) = (self.right().max(o.right()), self.bottom().max(o.bottom()));
        Rect::new(x0, y0, x1 - x0, y1 - y0)
    }
    pub fn expand(&self, d: i32) -> Rect {
        Rect::new(self.x - d, self.y - d, self.w + 2 * d, self.h + 2 * d)
    }
    pub fn offset(&self, dx: i32, dy: i32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

// ---- Text metrics ----

pub fn glyph_width(size: RasterHeight, weight: FontWeight) -> usize {
    get_raster_width(weight, size)
}

pub fn text_width(s: &str, size: RasterHeight, weight: FontWeight, spacing: usize) -> usize {
    let n = s.chars().count();
    if n == 0 { 0 } else { n * (glyph_width(size, weight) + spacing) - spacing }
}

/// Blend `src` over `dst` (both 0xRRGGBB) with coverage `a` (0..=255).
#[inline]
pub fn mix_rgb(dst: u32, src: u32, a: u32) -> u32 {
    let inv = 255 - a;
    let ch = |shift: u32| ((((src >> shift) & 0xFF) * a + ((dst >> shift) & 0xFF) * inv + 127) / 255) << shift;
    ch(16) | ch(8) | ch(0)
}

/// Straight-alpha "over": paint `src` (alpha `sa`) on top of `dst` (alpha `da`).
#[inline]
pub fn over(dst: u32, src: u32, sa: u32) -> u32 {
    let da = dst >> 24;
    if sa >= 255 || da == 0 {
        return (sa.min(255) << 24) | (src & 0xFF_FFFF);
    }
    if da == 255 {
        return 0xFF00_0000 | mix_rgb(dst, src, sa);
    }
    let rest = da * (255 - sa) / 255;
    let oa = sa + rest;
    if oa == 0 {
        return 0;
    }
    let ch = |shift: u32| ((((src >> shift) & 0xFF) * sa + ((dst >> shift) & 0xFF) * rest) / oa) << shift;
    oa << 24 | ch(16) | ch(8) | ch(0)
}

/// Signed distance from a point to a rounded rectangle at the origin (negative inside).
pub fn rounded_rect_sdf(px: f32, py: f32, w: f32, h: f32, r: f32) -> f32 {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let qx = (px - hw).abs() - (hw - r);
    let qy = (py - hh).abs() - (hh - r);
    let outside = if qx > 0.0 && qy > 0.0 { libm::sqrtf(qx * qx + qy * qy) } else { qx.max(qy).max(0.0) };
    outside + qx.max(qy).min(0.0) - r
}

/// Anything with pixels we can draw on.
pub trait Surface {
    fn size(&self) -> (usize, usize);
    fn put(&mut self, x: usize, y: usize, c: Rgb);
    fn get(&self, x: usize, y: usize) -> Rgb;

    /// Paint `c` over the pixel with coverage `alpha` (0.0 to 1.0).
    fn blend(&mut self, x: i64, y: i64, c: Rgb, alpha: f32) {
        let (w, h) = self.size();
        if x < 0 || y < 0 || alpha <= 0.0 || x as usize >= w || y as usize >= h {
            return;
        }
        let (x, y) = (x as usize, y as usize);
        if alpha >= 1.0 {
            self.put(x, y, c);
        } else {
            let under = self.get(x, y);
            self.put(x, y, under.mix(c, alpha));
        }
    }

    fn rect(&mut self, x: usize, y: usize, w: usize, h: usize, c: Rgb) {
        let (sw, sh) = self.size();
        for yy in y..(y + h).min(sh) {
            for xx in x..(x + w).min(sw) {
                self.put(xx, yy, c);
            }
        }
    }

    /// A smooth line (Xiaolin Wu's algorithm).
    fn line(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb, alpha: f32) {
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
        let mut plot = |x: f32, y: f32, a: f32| {
            let (px, py) = if steep { (y, x) } else { (x, y) };
            self.blend(px as i64, py as i64, c, a * alpha);
        };

        let xend = libm::roundf(x0);
        let yend = y0 + grad * (xend - x0);
        let xgap = rfpart(x0 + 0.5);
        let ypxl1 = libm::floorf(yend);
        plot(xend, ypxl1, rfpart(yend) * xgap);
        plot(xend, ypxl1 + 1.0, fpart(yend) * xgap);
        let xpxl1 = xend;
        let mut intery = yend + grad;

        let xend = libm::roundf(x1);
        let yend = y1 + grad * (xend - x1);
        let xgap = fpart(x1 + 0.5);
        let ypxl2 = libm::floorf(yend);
        plot(xend, ypxl2, rfpart(yend) * xgap);
        plot(xend, ypxl2 + 1.0, fpart(yend) * xgap);
        let xpxl2 = xend;

        let mut x = xpxl1 + 1.0;
        while x < xpxl2 {
            let fy = libm::floorf(intery);
            plot(x, fy, rfpart(intery));
            plot(x, fy + 1.0, fpart(intery));
            intery += grad;
            x += 1.0;
        }
    }

    /// A slightly heavier smooth line, used for outlines and clock hands.
    fn stroke(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, c: Rgb) {
        let (dx, dy) = (x1 - x0, y1 - y0);
        let len = libm::sqrtf(dx * dx + dy * dy).max(0.001);
        let (nx, ny) = (-dy / len * 0.33, dx / len * 0.33);
        self.line(x0 + nx, y0 + ny, x1 + nx, y1 + ny, c, 0.75);
        self.line(x0 - nx, y0 - ny, x1 - nx, y1 - ny, c, 0.75);
    }

    /// A smooth circle outline.
    fn ring(&mut self, cx: f32, cy: f32, r: f32, width: f32, c: Rgb) {
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
    fn disc(&mut self, cx: f32, cy: f32, r: f32, c: Rgb) {
        self.disc_alpha(cx, cy, r, c, 1.0);
    }

    fn disc_alpha(&mut self, cx: f32, cy: f32, r: f32, c: Rgb, alpha: f32) {
        let reach = (r + 2.0) as i64;
        for y in -reach..=reach {
            for x in -reach..=reach {
                let (px, py) = (cx as i64 + x, cy as i64 + y);
                let (dx, dy) = (px as f32 + 0.5 - cx, py as f32 + 0.5 - cy);
                let d = libm::sqrtf(dx * dx + dy * dy);
                self.blend(px, py, c, (r + 0.5 - d).clamp(0.0, 1.0) * alpha);
            }
        }
    }

    /// A filled polygon, anti-aliased by 4×4 supersampling.
    fn polygon(&mut self, pts: &[(f32, f32)], c: Rgb, alpha: f32) {
        if pts.len() < 3 {
            return;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for &(x, y) in pts {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        let inside = |sx: f32, sy: f32| {
            let mut odd = false;
            let mut j = pts.len() - 1;
            for i in 0..pts.len() {
                let (xi, yi) = pts[i];
                let (xj, yj) = pts[j];
                if (yi > sy) != (yj > sy) && sx < (xj - xi) * (sy - yi) / (yj - yi) + xi {
                    odd = !odd;
                }
                j = i;
            }
            odd
        };
        for py in libm::floorf(y0) as i64..=libm::ceilf(y1) as i64 {
            for px in libm::floorf(x0) as i64..=libm::ceilf(x1) as i64 {
                let mut n = 0;
                for sy in 0..4 {
                    for sx in 0..4 {
                        if inside(px as f32 + (sx as f32 + 0.5) / 4.0, py as f32 + (sy as f32 + 0.5) / 4.0) {
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    self.blend(px, py, c, n as f32 / 16.0 * alpha);
                }
            }
        }
    }

    /// Draws text blended over what is underneath. Returns the x after the last glyph.
    fn text(
        &mut self,
        x: usize,
        y: usize,
        s: &str,
        size: RasterHeight,
        weight: FontWeight,
        fg: Rgb,
        spacing: usize,
        opacity: f32,
    ) -> usize {
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

    /// Text enlarged by `scale` with smooth (bilinear) sampling, for big display type.
    fn text_scaled(
        &mut self,
        x: f32,
        y: f32,
        s: &str,
        size: RasterHeight,
        weight: FontWeight,
        fg: Rgb,
        scale: f32,
        tracking: f32,
        opacity: f32,
    ) -> f32 {
        let mut cx = x;
        for ch in s.chars() {
            let glyph = get_raster(ch, weight, size).or_else(|| get_raster('?', weight, size));
            let Some(glyph) = glyph else { continue };
            let rows: Vec<&[u8]> = glyph.rows().collect();
            let (gw, gh) = (glyph.width() as i64, rows.len() as i64);
            let sample = |sx: i64, sy: i64| -> f32 {
                if sx < 0 || sy < 0 || sx >= gw || sy >= gh { 0.0 } else { rows[sy as usize][sx as usize] as f32 }
            };
            let (ow, oh) = (libm::ceilf(gw as f32 * scale) as i64, libm::ceilf(gh as f32 * scale) as i64);
            let (bx, by) = (libm::floorf(cx) as i64, libm::floorf(y) as i64);
            for oy in 0..oh {
                let sy = (oy as f32 + 0.5) / scale - 0.5;
                let (y0, fy) = (libm::floorf(sy) as i64, sy - libm::floorf(sy));
                for ox in 0..ow {
                    let sx = (ox as f32 + 0.5) / scale - 0.5;
                    let (x0, fx) = (libm::floorf(sx) as i64, sx - libm::floorf(sx));
                    let top = sample(x0, y0) * (1.0 - fx) + sample(x0 + 1, y0) * fx;
                    let bottom = sample(x0, y0 + 1) * (1.0 - fx) + sample(x0 + 1, y0 + 1) * fx;
                    // Sharpen the enlarged edge, the way a distance-field font would.
                    let a = ((top * (1.0 - fy) + bottom * fy) / 255.0 - 0.22) / 0.46;
                    let a = a.clamp(0.0, 1.0);
                    let a = a * a * (3.0 - 2.0 * a);
                    if a > 0.004 {
                        self.blend(bx + ox, by + oy, fg, a * opacity);
                    }
                }
            }
            cx += gw as f32 * scale + tracking;
        }
        cx
    }
}

/// An off-screen image with an alpha channel (0xAARRGGBB). Windows, the
/// wallpaper and the launcher are each drawn into one, then composed.
pub struct Canvas {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u32>,
    clip: Option<Rect>,
}

impl Canvas {
    pub fn new(w: usize, h: usize, fill: u32) -> Self {
        Self { w, h, px: vec![fill; w * h], clip: None }
    }

    pub fn bounds(&self) -> Rect {
        Rect::new(0, 0, self.w as i32, self.h as i32)
    }

    /// Restrict drawing to `r` until `unclip`.
    pub fn clip(&mut self, r: Rect) {
        self.clip = Some(r.intersect(self.bounds()));
    }

    pub fn unclip(&mut self) {
        self.clip = None;
    }

    fn visible(&self, x: usize, y: usize) -> bool {
        x < self.w && y < self.h && self.clip.is_none_or(|c| c.contains(x as i32, y as i32))
    }

    /// Overwrite a rectangle with an exact 0xAARRGGBB value.
    pub fn fill(&mut self, r: Rect, argb: u32) {
        let r = r.intersect(self.clip.unwrap_or(self.bounds()));
        for y in r.y..r.bottom() {
            let start = y as usize * self.w + r.x as usize;
            self.px[start..start + r.w as usize].fill(argb);
        }
    }

    pub fn fill_rgb(&mut self, r: Rect, c: Rgb) {
        self.fill(r, c.argb(255));
    }

}

impl Canvas {
    /// A rounded panel: optional soft shadow, a fill (which may be translucent)
    /// and an optional hairline border, all anti-aliased.
    pub fn panel(&mut self, frame: Rect, radius: f32, fill: Rgb, fill_alpha: f32, border: Option<Rgb>, shadow: f32) {
        let reach = if shadow > 0.0 { 22 } else { 1 };
        let area = frame.expand(reach).union(frame.offset(0, 6).expand(reach)).intersect(self.clip.unwrap_or(self.bounds()));
        let (fw, fh) = (frame.w as f32, frame.h as f32);
        for py in area.y..area.bottom() {
            for px in area.x..area.right() {
                let (fx, fy) = ((px - frame.x) as f32 + 0.5, (py - frame.y) as f32 + 0.5);
                let d = rounded_rect_sdf(fx, fy, fw, fh, radius);
                let i = py as usize * self.w + px as usize;
                let mut v = self.px[i];
                if shadow > 0.0 && d > -0.5 {
                    let ds = rounded_rect_sdf(fx, fy - 5.0, fw, fh, radius);
                    let t = (1.0 - (ds + 3.0) / 18.0).clamp(0.0, 1.0);
                    let a = shadow * t * t * (d + 0.5).clamp(0.0, 1.0);
                    if a > 0.002 {
                        v = over(v, crate::theme::INK.argb(0), (a * 255.0) as u32);
                    }
                }
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    v = over(v, fill.argb(0), (cover * fill_alpha * 255.0 + 0.5) as u32);
                    if let Some(b) = border {
                        let edge = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
                        if edge > 0.0 {
                            v = over(v, b.argb(0), (edge * 255.0) as u32);
                        }
                    }
                }
                self.px[i] = v;
            }
        }
    }
}

impl Surface for Canvas {
    fn size(&self) -> (usize, usize) {
        (self.w, self.h)
    }

    fn put(&mut self, x: usize, y: usize, c: Rgb) {
        if self.visible(x, y) {
            self.px[y * self.w + x] = c.argb(255);
        }
    }

    fn get(&self, x: usize, y: usize) -> Rgb {
        if x < self.w && y < self.h { Rgb::from_u32(self.px[y * self.w + x]) } else { Rgb(0, 0, 0) }
    }

    fn blend(&mut self, x: i64, y: i64, c: Rgb, alpha: f32) {
        if x < 0 || y < 0 || alpha <= 0.0 || !self.visible(x as usize, y as usize) {
            return;
        }
        let a = (alpha.min(1.0) * 255.0 + 0.5) as u32;
        if a == 0 {
            return;
        }
        let i = y as usize * self.w + x as usize;
        self.px[i] = over(self.px[i], c.argb(0), a);
    }
}
