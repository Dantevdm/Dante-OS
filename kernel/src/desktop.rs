//! The First Light wallpaper: a still sea at dawn, drawn like a technical
//! drawing. A sun that breathes, waves that drift, and a small plane crossing
//! the sky once every 48 seconds.
//!
//! Everything is painted into the wallpaper canvas through `paint(region)`, so
//! any part of it can be repainted on its own when something moves.

use alloc::format;
use alloc::string::String;
use core::f32::consts::PI;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::boot_screen::ease_in_out;
use crate::gfx::{Canvas, Rect, Rgb, Surface, text_width};
use crate::theme::*;

const SUN: Rgb = Rgb::hex(0xF2CFB4);
const SEA_TOP: Rgb = Rgb::hex(0xD3DEE0);
const SEA_BOTTOM: Rgb = Rgb::hex(0xAFC3C9);
const WAVE: Rgb = Rgb::hex(0x9DB2B8);
pub const MARGIN: i32 = 56;

const PLANE_MS: u64 = 48_000;
const SUN_MS: u64 = 9_000;
const WAVE_MS: u64 = 16_000;
const WAVE_DRIFT: f32 = 22.0;

/// The plane, as drawn in the design (pointing along +x).
const PLANE: [(f32, f32); 6] = [(-9.0, -5.0), (5.0, -9.0), (1.0, -3.0), (9.0, 1.0), (-1.0, 0.0), (-7.0, 7.0)];

pub struct Desktop {
    w: i32,
    h: i32,
    pub horizon: i32,
    sun_x: f32,
    sun_r: f32,
    pub clock: String,
    pub open: usize,
}

impl Desktop {
    pub fn new(w: usize, h: usize) -> Self {
        let (w, h) = (w as i32, h as i32);
        Self { w, h, horizon: h * 2 / 3, sun_x: w as f32 * 1200.0 / 1440.0, sun_r: 74.0, clock: String::new(), open: 0 }
    }

    /// Design coordinates (1440×900) to screen coordinates.
    fn sx(&self, v: f32) -> f32 {
        v * self.w as f32 / 1440.0
    }
    fn sy(&self, v: f32) -> f32 {
        v * self.h as f32 / 900.0
    }

    fn sky(&self, y: i32) -> Rgb {
        SKY.at(y as f32 / self.h as f32)
    }

    fn sea(&self, y: i32) -> Rgb {
        SEA_TOP.mix(SEA_BOTTOM, (y - self.horizon) as f32 / (self.h - self.horizon).max(1) as f32)
    }

    fn path(&self) -> [(f32, f32); 4] {
        [(1160.0, 440.0), (1240.0, 380.0), (1330.0, 300.0), (1400.0, 210.0)].map(|(x, y)| (self.sx(x), self.sy(y)))
    }

    // ---- Regions that change on their own ----

    pub fn header_rect(&self) -> Rect {
        Rect::new(0, 18, self.w, 32)
    }

    pub fn footer_rect(&self) -> Rect {
        Rect::new(0, self.h - 64, self.w, 40)
    }

    pub fn sun_rect(&self) -> Rect {
        let r = self.sun_r as i32 + 2;
        Rect::new(self.sun_x as i32 - r, self.horizon - r, 2 * r, r)
    }

    fn waves(&self) -> [(f32, f32, f32); 3] {
        [(80.0, 680.0, 120.0), (300.0, 720.0, 80.0), (520.0, 660.0, 80.0)].map(|(x, y, l)| (self.sx(x), self.sy(y), self.sx(l)))
    }

    pub fn wave_rects(&self) -> [Rect; 3] {
        self.waves().map(|(x, y, l)| Rect::new(x as i32 - 1, y as i32, (l + WAVE_DRIFT) as i32 + 3, 1))
    }

    fn plane_pose(&self, now: u64) -> (f32, f32, f32, f32) {
        let t = (now % PLANE_MS) as f32 / PLANE_MS as f32;
        let e = ease_in_out(t);
        let p = self.path();
        let u = 1.0 - e;
        let x = u * u * u * p[0].0 + 3.0 * u * u * e * p[1].0 + 3.0 * u * e * e * p[2].0 + e * e * e * p[3].0;
        let y = u * u * u * p[0].1 + 3.0 * u * u * e * p[1].1 + 3.0 * u * e * e * p[2].1 + e * e * e * p[3].1;
        let dx = 3.0 * u * u * (p[1].0 - p[0].0) + 6.0 * u * e * (p[2].0 - p[1].0) + 3.0 * e * e * (p[3].0 - p[2].0);
        let dy = 3.0 * u * u * (p[1].1 - p[0].1) + 6.0 * u * e * (p[2].1 - p[1].1) + 3.0 * e * e * (p[3].1 - p[2].1);
        let fade = (t * 12.0).min((1.0 - t) * 12.0).clamp(0.0, 1.0);
        (x, y, libm::atan2f(dy, dx), fade)
    }

    pub fn plane_rect(&self, now: u64) -> Rect {
        let (x, y, _, _) = self.plane_pose(now);
        Rect::new(x as i32 - 13, y as i32 - 13, 27, 27)
    }

    /// Clicking DANTE in the header or "type anything" in the footer opens the launcher.
    pub fn launcher_hit(&self, x: i32, y: i32) -> bool {
        Rect::new(MARGIN - 8, 18, 96, 32).contains(x, y) || Rect::new(MARGIN - 8, self.h - 64, 220, 40).contains(x, y)
    }

    // ---- Painting ----

    pub fn paint(&self, c: &mut Canvas, region: Rect, now: u64) {
        let region = region.intersect(c.bounds());
        if region.is_empty() {
            return;
        }
        c.clip(region);
        let (w, h) = (self.w, self.h);

        // Sky, then the sun, then the sea in front of its lower half.
        for y in region.y..region.bottom().min(self.horizon) {
            c.fill_rgb(Rect::new(region.x, y, region.w, 1), self.sky(y));
        }
        let breathe = 0.825 - 0.125 * libm::cosf(2.0 * PI * (now % SUN_MS) as f32 / SUN_MS as f32);
        if region.intersects(self.sun_rect()) {
            c.disc_alpha(self.sun_x, self.horizon as f32, self.sun_r, SUN, breathe);
        }
        for y in region.y.max(self.horizon)..region.bottom() {
            c.fill_rgb(Rect::new(region.x, y, region.w, 1), self.sea(y));
        }

        // Horizon, with a ruler's ticks under the sun.
        c.fill_rgb(Rect::new(0, self.horizon, w, 1), RULE);
        let mut x = self.sun_x as i32 - 40;
        let mut i = 0;
        while x < w {
            let len = if i == 1 { 12 } else { 8 };
            c.fill_rgb(Rect::new(x, self.horizon + 6, 1, len), RULE);
            x += 40;
            i += 1;
        }

        // Dimension line across the sun, with its label.
        let dim_y = self.sy(500.0) as i32;
        let (x0, x1) = ((self.sun_x - self.sun_r) as i32, (self.sun_x + self.sun_r) as i32);
        if region.intersects(Rect::new(x0 - 40, dim_y - 30, x1 - x0 + 80, 44)) {
            c.fill_rgb(Rect::new(x0, dim_y, x1 - x0, 1), RULE);
            c.fill_rgb(Rect::new(x0, dim_y - 6, 1, 13), RULE);
            c.fill_rgb(Rect::new(x1, dim_y - 6, 1, 13), RULE);
            let label = "Ø 148 · first light";
            let lw = text_width(label, RasterHeight::Size16, FontWeight::Light, 0) as i32;
            c.text((self.sun_x as i32 - lw / 2) as usize, (dim_y - 24) as usize, label, RasterHeight::Size16, FontWeight::Light, MUTED, 0, 1.0);
        }

        // The plane's flight path, dashed, and the plane on it.
        let p = self.path();
        let path_area = Rect::new(p[0].0 as i32 - 4, p[3].1 as i32 - 4, (p[3].0 - p[0].0) as i32 + 8, (p[0].1 - p[3].1) as i32 + 8);
        if region.intersects(path_area) {
            let mut prev = p[0];
            let mut run = 0.0;
            for k in 1..=300 {
                let t = k as f32 / 300.0;
                let u = 1.0 - t;
                let pt = (
                    u * u * u * p[0].0 + 3.0 * u * u * t * p[1].0 + 3.0 * u * t * t * p[2].0 + t * t * t * p[3].0,
                    u * u * u * p[0].1 + 3.0 * u * u * t * p[1].1 + 3.0 * u * t * t * p[2].1 + t * t * t * p[3].1,
                );
                let (ddx, ddy) = (pt.0 - prev.0, pt.1 - prev.1);
                if run % 8.0 < 2.0 {
                    c.line(prev.0, prev.1, pt.0, pt.1, RULE, 0.9);
                }
                run += libm::sqrtf(ddx * ddx + ddy * ddy);
                prev = pt;
            }
        }
        let (px, py, angle, fade) = self.plane_pose(now);
        if fade > 0.0 && region.intersects(self.plane_rect(now)) {
            let (s, co) = (libm::sinf(angle), libm::cosf(angle));
            let pts = PLANE.map(|(x, y)| (px + x * co - y * s, py + x * s + y * co));
            c.polygon(&pts, SEA, fade);
        }

        // Waves, drifting slowly back and forth.
        let drift = (1.0 - libm::cosf(2.0 * PI * (now % WAVE_MS) as f32 / WAVE_MS as f32)) / 2.0 * WAVE_DRIFT;
        for (x, y, l) in self.waves() {
            let x = x + drift;
            let frac = x - libm::floorf(x);
            // Soft sub-pixel ends keep the slow motion smooth.
            c.blend(x as i64, y as i64, WAVE, 1.0 - frac);
            c.fill_rgb(Rect::new(x as i32 + 1, y as i32, l as i32 - 1, 1), WAVE);
            c.blend(x as i64 + l as i64, y as i64, WAVE, frac);
        }

        // Header: DANTE · workspace · clock.
        let f = RasterHeight::Size16;
        if region.intersects(self.header_rect()) {
            c.text(MARGIN as usize, 26, "DANTE", f, FontWeight::Regular, INK, 2, 1.0);
            let ws = "work";
            let ww = text_width(ws, f, FontWeight::Regular, 1) as i32;
            c.text((w / 2 - ww / 2) as usize, 26, ws, f, FontWeight::Regular, INK, 1, 1.0);
            let cw = text_width(&self.clock, f, FontWeight::Regular, 1) as i32;
            c.text((w - MARGIN - cw) as usize, 26, &self.clock, f, FontWeight::Regular, MUTED, 1, 1.0);
        }

        // Footer, resting on the water.
        if region.intersects(self.footer_rect()) {
            let y = h - 56;
            c.disc(MARGIN as f32 + 4.0, y as f32 + 9.0, 4.5, SEA);
            c.text(MARGIN as usize + 20, y as usize, "type anything", f, FontWeight::Regular, INK, 0, 1.0);
            let status = match self.open {
                0 => String::from("nothing open"),
                n => format!("{n} open"),
            };
            let sw = text_width(&status, f, FontWeight::Regular, 0) as i32;
            c.text((w - MARGIN - sw) as usize, y as usize, &status, f, FontWeight::Regular, INK, 0, 1.0);
        }

        c.unclip();
    }
}
