//! Clock: a dial drawn like an instrument, and the time in words underneath.

use alloc::format;
use core::f32::consts::PI;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use super::{App, AppKind};
use crate::gfx::{Canvas, Rect, Surface, text_width};
use crate::theme::*;
use crate::time;

pub struct Clock {
    second: u8,
    last_check: u64,
}

impl Clock {
    pub fn new() -> Self {
        Self { second: 255, last_check: 0 }
    }
}

impl App for Clock {
    fn kind(&self) -> AppKind {
        AppKind::Clock
    }

    fn size(&self) -> (i32, i32) {
        (360, 440)
    }

    fn subtitle(&self) -> &str {
        "local time"
    }

    fn draw(&mut self, c: &mut Canvas, area: Rect) {
        c.fill_rgb(area, PAPER);
        let now = time::now();
        self.second = now.second;

        let r = (area.w as f32 / 2.0 - 16.0).min(124.0);
        let (cx, cy) = (area.x as f32 + area.w as f32 / 2.0, area.y as f32 + r + 8.0);

        // The dial: a hairline ring with minute and hour ticks.
        c.ring(cx, cy, r, 1.0, HAIRLINE);
        for i in 0..60 {
            let a = i as f32 * PI / 30.0;
            let (s, co) = (libm::sinf(a), libm::cosf(a));
            let (inner, colour) = if i % 5 == 0 { (r - 12.0, RULE) } else { (r - 5.0, HAIRLINE) };
            c.line(cx + s * inner, cy - co * inner, cx + s * (r - 2.0), cy - co * (r - 2.0), colour, 1.0);
        }

        // Hands.
        let hand = |turns: f32, len: f32, tail: f32| {
            let a = turns * 2.0 * PI;
            let (s, co) = (libm::sinf(a), libm::cosf(a));
            (cx - s * tail, cy + co * tail, cx + s * len, cy - co * len)
        };
        let h = (now.hour % 12) as f32 + now.minute as f32 / 60.0;
        let m = now.minute as f32 + now.second as f32 / 60.0;
        let (x0, y0, x1, y1) = hand(h / 12.0, r * 0.5, 0.0);
        c.stroke(x0, y0, x1, y1, INK);
        c.stroke(x0 + 0.4, y0 + 0.4, x1 + 0.4, y1 + 0.4, INK);
        let (x0, y0, x1, y1) = hand(m / 60.0, r * 0.76, 0.0);
        c.stroke(x0, y0, x1, y1, INK);
        let (x0, y0, x1, y1) = hand(now.second as f32 / 60.0, r * 0.86, 16.0);
        c.line(x0, y0, x1, y1, TERRACOTTA, 1.0);
        c.disc(cx, cy, 4.5, SEA);
        c.disc(cx, cy, 1.5, PAPER);

        // The time, in large light type, and the date.
        let t = format!("{:02}:{:02}:{:02}", now.hour, now.minute, now.second);
        let tw = text_width(&t, RasterHeight::Size32, FontWeight::Light, 0) as f32;
        let ty = cy + r + 26.0;
        c.text((cx - tw / 2.0) as usize, ty as usize, &t, RasterHeight::Size32, FontWeight::Light, INK, 0, 1.0);
        let d = now.date_text();
        let dw = text_width(&d, RasterHeight::Size16, FontWeight::Regular, 0) as f32;
        c.text((cx - dw / 2.0) as usize, (ty + 42.0) as usize, &d, RasterHeight::Size16, FontWeight::Regular, MUTED, 0, 1.0);
    }

    fn tick(&mut self, now: u64, c: &mut Canvas, area: Rect) -> Option<Rect> {
        if now - self.last_check < 200 {
            return None;
        }
        self.last_check = now;
        if time::now().second == self.second {
            return None;
        }
        self.draw(c, area);
        Some(area)
    }
}
