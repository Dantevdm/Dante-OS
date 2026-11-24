//! System: what this machine is, updated once a second.

use alloc::format;
use alloc::string::String;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use super::{App, AppKind, DISPLAY_H, DISPLAY_W};
use crate::gfx::{Canvas, Rect, Rgb, Surface, glyph_width, text_width};
use crate::theme::*;
use crate::{VERSION, heap, memory};
use core::sync::atomic::Ordering;

const ROW_H: i32 = 34;
const DIVIDER: Rgb = Rgb::hex(0xDCE2E2);

pub struct System {
    brand: String,
    last: u64,
}

impl System {
    pub fn new() -> Self {
        Self { brand: crate::cpu::names().1, last: 0 }
    }
}

impl App for System {
    fn kind(&self) -> AppKind {
        AppKind::System
    }

    fn size(&self) -> (i32, i32) {
        (460, 480)
    }

    fn draw(&mut self, c: &mut Canvas, area: Rect) {
        c.fill_rgb(area, PAPER);
        let (x, y) = (area.x as usize, area.y as usize);
        c.text(x, y, "Dante OS", RasterHeight::Size32, FontWeight::Light, INK, 0, 1.0);
        c.text(x, y + 40, &format!("Version {VERSION}"), RasterHeight::Size16, FontWeight::Regular, MUTED, 0, 1.0);

        let (used, total) = memory::stats();
        let (heap_used, heap_free) = heap::stats();
        let disk = match crate::fs::usage() {
            Some((used, total)) => format!("{} of {}", crate::fs::size_text(used), crate::fs::size_text(total)),
            None => String::from("None"),
        };
        let rows: [(&str, String); 7] = [
            ("Processor", self.brand.clone()),
            ("Cores", format!("{}", super::cores())),
            ("Memory", format!("{} of {} MiB", used * 4 / 1024, total * 4 / 1024)),
            ("Kernel heap", format!("{} KiB of {} MiB", heap_used / 1024, (heap_used + heap_free) / (1024 * 1024))),
            ("Disk", disk),
            ("Display", format!("{} × {}", DISPLAY_W.load(Ordering::Relaxed), DISPLAY_H.load(Ordering::Relaxed))),
            ("Uptime", super::uptime_text()),
        ];
        let gw = glyph_width(RasterHeight::Size16, FontWeight::Regular) as i32;
        let max_chars = ((area.w - 120) / gw).max(4) as usize;
        let mut ry = area.y + 84;
        for (label, value) in rows.iter() {
            c.fill_rgb(Rect::new(area.x, ry, area.w, 1), DIVIDER);
            c.text(x, (ry + 9) as usize, label, RasterHeight::Size16, FontWeight::Regular, MUTED, 0, 1.0);
            let value: String = value.chars().take(max_chars).collect();
            let vw = text_width(&value, RasterHeight::Size16, FontWeight::Regular, 0) as i32;
            c.text((area.right() - vw) as usize, (ry + 9) as usize, &value, RasterHeight::Size16, FontWeight::Regular, INK, 0, 1.0);
            ry += ROW_H;
        }
        c.fill_rgb(Rect::new(area.x, ry, area.w, 1), DIVIDER);

        // Memory as a ruler: ticks every tenth, filled up to what's in use.
        let by = ry + 30;
        c.fill_rgb(Rect::new(area.x, by, area.w, 1), HAIRLINE);
        for i in 0..=10 {
            let tx = area.x + (area.w - 1) * i / 10;
            let len = if i % 5 == 0 { 9 } else { 5 };
            c.fill_rgb(Rect::new(tx, by - len, 1, len), RULE);
        }
        let fill = if total == 0 { 0 } else { ((used as i64 * area.w as i64) / total as i64).max(2) as i32 };
        c.fill_rgb(Rect::new(area.x, by - 1, fill, 3), SEA);
    }

    fn tick(&mut self, now: u64, c: &mut Canvas, area: Rect) -> Option<Rect> {
        if now - self.last < 1000 {
            return None;
        }
        self.last = now;
        self.draw(c, area);
        Some(area)
    }
}
