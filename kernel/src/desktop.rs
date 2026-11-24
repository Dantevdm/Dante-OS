//! The First Light desktop: a still sea at dawn, drawn like a technical
//! drawing, with one window on it.

use core::fmt::Write;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::boot_screen::ease_out;
use crate::screen::{Buf, Rgb, Screen};
use crate::theme::*;
use crate::time;

const SUN: Rgb = Rgb::hex(0xF2CFB4);
const SEA_TOP: Rgb = Rgb::hex(0xD3DEE0);
const SEA_BOTTOM: Rgb = Rgb::hex(0xAFC3C9);
const WAVE: Rgb = Rgb::hex(0x9DB2B8);
const MARGIN: usize = 56;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub w: usize,
    pub h: usize,
}

pub struct Desktop {
    pub horizon: usize,
    pub window: Rect,
}

impl Desktop {
    pub fn new(s: &Screen) -> Self {
        let horizon = s.height * 72 / 100;
        let window = Rect { x: MARGIN, y: 64, w: (s.width * 62 / 100).max(560), h: horizon - 64 - 28 };
        Self { horizon, window }
    }

    /// The colour of the wallpaper at a given row (sky above the horizon, sea below).
    fn wallpaper_row(&self, s: &Screen, y: usize) -> Rgb {
        if y < self.horizon {
            s.background_at(y)
        } else {
            let t = (y - self.horizon) as f32 / (s.height - self.horizon).max(1) as f32;
            SEA_TOP.mix(SEA_BOTTOM, t)
        }
    }

    pub fn draw_wallpaper(&self, s: &mut Screen) {
        let (w, h) = (s.width, s.height);
        s.clear_rect(0, 0, w, self.horizon);

        // The sun, half risen, sits behind the sea.
        let sun_x = w as f32 * 0.83;
        s.disc(sun_x, self.horizon as f32, 74.0, SUN);
        for y in self.horizon..h {
            let c = self.wallpaper_row(s, y);
            s.rect(0, y, w, 1, c);
        }

        // Horizon line with a ruler's ticks under the sun.
        s.rect(0, self.horizon, w, 1, RULE);
        let mut x = (sun_x as usize).saturating_sub(40);
        let mut i = 0;
        while x < w {
            let len = if i == 1 { 12 } else { 8 };
            s.rect(x, self.horizon + 6, 1, len, RULE);
            x += 40;
            i += 1;
        }

        // Dimension line across the sun: "Ø 148 · hh:mm".
        let dim_y = self.horizon - 100;
        let (x0, x1) = (sun_x as usize - 74, sun_x as usize + 74);
        s.rect(x0, dim_y, x1 - x0, 1, RULE);
        s.rect(x0, dim_y - 6, 1, 13, RULE);
        s.rect(x1, dim_y - 6, 1, 13, RULE);

        // A few calm wave strokes.
        for (wx, wy, ww) in [(80, 80, 120), (300, 120, 80), (520, 60, 80), (760, 100, 60)] {
            s.rect(wx, self.horizon + wy, ww, 1, WAVE);
        }
    }

    /// Header: DANTE · workspace · clock.
    pub fn draw_header(&self, s: &mut Screen) {
        let w = s.width;
        s.clear_rect(0, 24, w, 24);
        let f = RasterHeight::Size16;
        s.text(MARGIN, 26, "DANTE", f, FontWeight::Regular, INK, 2, 1.0);
        let ws = "work";
        s.text(w / 2 - Screen::text_width(ws, f, FontWeight::Regular, 1) / 2, 26, ws, f, FontWeight::Regular, INK, 1, 1.0);

        let now = time::now();
        let mut clock: Buf<16> = Buf::new();
        let _ = write!(clock, "{:02}:{:02}", now.hour, now.minute);
        let cw = Screen::text_width(clock.as_str(), f, FontWeight::Regular, 1);
        s.text(w - MARGIN - cw, 26, clock.as_str(), f, FontWeight::Regular, MUTED, 1, 1.0);
    }

    pub fn draw_footer(&self, s: &mut Screen, hint: &str) {
        let y = s.height - 44;
        for yy in y - 4..y + 22 {
            let c = self.wallpaper_row(s, yy);
            s.rect(0, yy, s.width, 1, c);
        }
        s.disc(MARGIN as f32 + 4.0, y as f32 + 9.0, 4.5, SEA);
        s.text(MARGIN + 20, y, hint, RasterHeight::Size16, FontWeight::Regular, INK, 0, 1.0);
    }

    /// Draws the window frame and title. `t` (0..1) animates it rising into place.
    pub fn draw_window(&self, s: &mut Screen, title: &str, t: f32) {
        let r = self.window;
        let e = ease_out(t);
        let lift = ((1.0 - e) * 14.0) as usize;

        // Restore the wallpaper the window may have covered on the previous frame.
        for y in r.y.saturating_sub(8)..(r.y + r.h + 22).min(self.horizon) {
            let c = self.wallpaper_row(s, y);
            s.rect(r.x - 8, y, r.w + 16, 1, c);
        }

        let (x, y) = (r.x as i64, (r.y + lift) as i64);
        s.rounded_rect(x, y, r.w as i64, r.h as i64, 10.0, PAPER, 0.94 * e, Some(HAIRLINE.mix(PAPER, 1.0 - e)));

        if t >= 1.0 {
            // Technical-drawing corner marks.
            let (x, y) = (r.x - 5, r.y - 5);
            s.rect(x, y, 9, 1, RULE);
            s.rect(x, y, 1, 9, RULE);
            let (x2, y2) = (r.x + r.w + 4, r.y + r.h + 4);
            s.rect(x2 - 8, y2, 9, 1, RULE);
            s.rect(x2, y2 - 8, 1, 9, RULE);
        }
        s.text(r.x + 28, r.y + lift + 20, title, RasterHeight::Size16, FontWeight::Regular, MUTED, 0, e);
    }

    /// The window's paper colour at row `y`, for redrawing text areas.
    pub fn paper_at(&self, s: &Screen, y: usize) -> Rgb {
        self.wallpaper_row(s, y).mix(PAPER, 0.94)
    }
}
