//! The First Light boot screen: a gear that turns while the system starts,
//! the DANTE wordmark fading up, and a ruler that fills as boot progresses.

use core::f32::consts::PI;
use core::fmt::Write;

use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::screen::{Buf, Screen};
use crate::theme::*;

const GEAR_SIZE: f32 = 132.0; // px, matching the design
const RULER_W: usize = 320;
const WORD: &str = "DANTE";
const WORD_SPACING: usize = 18;

pub struct BootScreen {
    cx: f32,
    gear_top: usize,
    word_y: usize,
    ruler_x: usize,
    ruler_y: usize,
}

impl BootScreen {
    pub fn new(s: &Screen) -> Self {
        let cx = s.width as f32 / 2.0;
        let gear_top = s.height * 29 / 100;
        let word_y = gear_top + GEAR_SIZE as usize + 34;
        let ruler_y = word_y + 32 + 40;
        Self { cx, gear_top, word_y, ruler_x: s.width / 2 - RULER_W / 2, ruler_y }
    }

    /// Paints the static parts: background, ruler scale and the boot log.
    pub fn draw_base(&self, s: &mut Screen, lines: &[&str]) {
        s.clear();

        // Ruler: a baseline with a tick every 10%, taller every 50%.
        let base = self.ruler_y + 12;
        s.rect(self.ruler_x, base, RULER_W + 1, 1, RULE);
        for i in 0..=10 {
            let h = if i % 5 == 0 { 12 } else { 6 };
            s.rect(self.ruler_x + i * RULER_W / 10, base + 1 - h, 1, h, RULE);
        }
        let small = RasterHeight::Size16;
        s.text(self.ruler_x, base + 8, "0", small, FontWeight::Regular, MUTED, 0, 1.0);
        let w100 = Screen::text_width("100", small, FontWeight::Regular, 0);
        s.text(self.ruler_x + RULER_W - w100, base + 8, "100", small, FontWeight::Regular, MUTED, 0, 1.0);

        // Boot log, bottom left.
        let mut y = s.height - 44 - lines.len() * 23;
        for line in lines {
            s.text(56, y, line, small, FontWeight::Regular, MUTED, 0, 1.0);
            y += 23;
        }
    }

    /// One animation frame. `t` runs from 0.0 to 1.0 over the boot.
    pub fn frame(&self, s: &mut Screen, t: f32) {
        self.draw_gear(s, t * PI / 3.0); // a slow sixth of a turn over the boot

        // Wordmark: fades up and rises 10 px during the first third.
        let wt = ease_out((t * 3.0).clamp(0.0, 1.0));
        let word_w = Screen::text_width(WORD, RasterHeight::Size32, FontWeight::Regular, WORD_SPACING);
        let wx = self.cx as usize - word_w / 2;
        s.clear_rect(wx, self.word_y, word_w + 2, 32 + 12);
        let lift = ((1.0 - wt) * 10.0) as usize;
        s.text(wx, self.word_y + lift, WORD, RasterHeight::Size32, FontWeight::Regular, INK, WORD_SPACING, wt);

        // Ruler fill: 2 px of sea along the baseline, eased.
        let fill = (ease_in_out(t) * RULER_W as f32) as usize;
        s.rect(self.ruler_x, self.ruler_y + 11, fill, 2, SEA);
    }

    pub fn ready(&self, s: &mut Screen) {
        let y = s.height - 44;
        s.text(56, y, "ready", RasterHeight::Size16, FontWeight::Regular, SEA, 0, 1.0);
    }

    fn draw_gear(&self, s: &mut Screen, angle: f32) {
        let scale = GEAR_SIZE / 120.0; // the design's gear lives in a 120-unit box
        let (cx, cy) = (self.cx, self.gear_top as f32 + GEAR_SIZE / 2.0);
        let half = GEAR_SIZE as usize / 2 + 4;
        s.clear_rect(cx as usize - half, cy as usize - half, half * 2, half * 2);

        // Twelve teeth: each is four points between the root (r) and tip (R) radius.
        let (teeth, r_root, r_tip) = (12, 42.0 * scale, 52.0 * scale);
        let step = 2.0 * PI / teeth as f32;
        let mut prev: Option<(f32, f32)> = None;
        let mut first = (0.0, 0.0);
        for i in 0..teeth {
            let a = angle + i as f32 * step;
            for (rad, off) in [(r_root, 0.0), (r_tip, 0.18), (r_tip, 0.5), (r_root, 0.68)] {
                let p = (cx + rad * libm::cosf(a + off * step), cy + rad * libm::sinf(a + off * step));
                match prev {
                    Some(q) => s.stroke(q.0, q.1, p.0, p.1, INK),
                    None => first = p,
                }
                prev = Some(p);
            }
        }
        if let Some(q) = prev {
            s.stroke(q.0, q.1, first.0, first.1, INK);
        }

        // Hub, a pointer that turns with the gear, and a still sea-coloured centre.
        s.ring(cx, cy, 20.0 * scale, 1.3, INK);
        let pa = angle - PI / 2.0;
        s.stroke(cx, cy, cx + 20.0 * scale * libm::cosf(pa), cy + 20.0 * scale * libm::sinf(pa), SEA);
        s.disc(cx, cy, 5.0 * scale, SEA);
    }
}

pub fn ease_out(t: f32) -> f32 {
    1.0 - (1.0 - t) * (1.0 - t) * (1.0 - t)
}

pub fn ease_in_out(t: f32) -> f32 {
    if t < 0.5 { 4.0 * t * t * t } else { 1.0 - libm::powf(-2.0 * t + 2.0, 3.0) / 2.0 }
}

/// Formats the hardware summary lines for the boot log.
pub fn summary(usable_mib: u64, cores: usize) -> (Buf<64>, Buf<64>) {
    let mut l1: Buf<64> = Buf::new();
    let _ = write!(l1, "kernel {} \u{b7} x86-64 \u{b7} UEFI", crate::VERSION);
    let mut l2: Buf<64> = Buf::new();
    let _ = write!(l2, "{usable_mib} MiB usable \u{b7} {cores} cores online");
    (l1, l2)
}
