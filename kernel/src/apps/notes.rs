//! Notes: a ruled page to write on. Kept in memory until Dante has a disk.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use super::{App, AppKind, BLINK_MS, Request};
use crate::gfx::{Canvas, Rect, Rgb, Surface, glyph_width};
use crate::keyboard::Key;
use crate::theme::*;
use crate::time;

const FONT: RasterHeight = RasterHeight::Size16;
const LINE_H: i32 = 28;
const RULED: Rgb = Rgb::hex(0xE2E7E6);
const MAX_CHARS: usize = 8000;
const FOOTER_H: i32 = 30;

pub struct Notes {
    text: String,
    caret: Rect,
    caret_on: bool,
    focused: bool,
    last_blink: u64,
}

impl Notes {
    pub fn new() -> Self {
        Self { text: String::new(), caret: Rect::default(), caret_on: true, focused: true, last_blink: 0 }
    }

    /// The text broken into lines that fit `cols`.
    fn wrapped(&self, cols: usize) -> Vec<String> {
        let mut lines = Vec::new();
        for para in self.text.split('\n') {
            let mut line = String::new();
            let mut n = 0;
            for word in para.split_inclusive(' ') {
                let len = word.chars().count();
                if n + len > cols && n > 0 {
                    lines.push(core::mem::take(&mut line));
                    n = 0;
                }
                for ch in word.chars() {
                    if n == cols {
                        lines.push(core::mem::take(&mut line));
                        n = 0;
                    }
                    line.push(ch);
                    n += 1;
                }
            }
            lines.push(line);
        }
        lines
    }

    fn draw_caret(&self, c: &mut Canvas) -> Rect {
        let colour = if self.caret_on && self.focused { SEA } else { PAPER };
        c.fill_rgb(self.caret, colour);
        self.caret
    }
}

impl App for Notes {
    fn kind(&self) -> AppKind {
        AppKind::Notes
    }

    fn size(&self) -> (i32, i32) {
        (500, 450)
    }

    fn subtitle(&self) -> &str {
        "untitled"
    }

    fn draw(&mut self, c: &mut Canvas, area: Rect) {
        c.fill_rgb(area, PAPER);
        let gw = glyph_width(FONT, FontWeight::Regular) as i32;
        let cols = (area.w / gw).max(8) as usize;
        let page = Rect::new(area.x, area.y, area.w, area.h - FOOTER_H);
        let rows = (page.h / LINE_H) as usize;

        // Ruled lines, like a notebook.
        for r in 0..rows {
            c.fill_rgb(Rect::new(page.x, page.y + (r as i32 + 1) * LINE_H - 4, page.w, 1), RULED);
        }

        let lines = self.wrapped(cols);
        let first = lines.len().saturating_sub(rows);
        let mut caret_x = page.x;
        let mut caret_y = page.y;
        for (i, line) in lines[first..].iter().enumerate() {
            let y = page.y + i as i32 * LINE_H + 5;
            let end = c.text(page.x as usize, y as usize, line, FONT, FontWeight::Regular, INK, 0, 1.0);
            caret_x = end as i32;
            caret_y = y;
        }
        self.caret = Rect::new(caret_x + 1, caret_y, 2, 17);
        self.draw_caret(c);

        // Footer: a word count and an honest note about saving.
        let words = self.text.split_whitespace().count();
        let note = if self.text.is_empty() {
            String::from("start typing · kept in memory, no disk yet")
        } else {
            format!("{words} word{} · kept in memory, no disk yet", if words == 1 { "" } else { "s" })
        };
        let fy = area.bottom() - FOOTER_H;
        c.fill_rgb(Rect::new(area.x, fy + 6, area.w, 1), HAIRLINE);
        c.text(area.x as usize, (fy + 13) as usize, &note, FONT, FontWeight::Light, MUTED, 0, 1.0);
    }

    fn key(&mut self, key: Key, c: &mut Canvas, area: Rect, _out: &mut Vec<Request>) -> Option<Rect> {
        self.caret_on = true;
        self.last_blink = time::millis();
        match key {
            Key::Char('\u{8}') => {
                self.text.pop();
            }
            Key::Char('\t') => self.text.push_str("    "),
            Key::Char(ch) if (ch == '\n' || !ch.is_control()) && self.text.len() < MAX_CHARS => self.text.push(ch),
            _ => return None,
        }
        self.draw(c, area);
        Some(area)
    }

    fn tick(&mut self, now: u64, c: &mut Canvas, _area: Rect) -> Option<Rect> {
        if !self.focused || now - self.last_blink < BLINK_MS {
            return None;
        }
        self.last_blink = now;
        self.caret_on = !self.caret_on;
        Some(self.draw_caret(c))
    }

    fn focus(&mut self, focused: bool, c: &mut Canvas, _area: Rect) -> Option<Rect> {
        self.focused = focused;
        self.caret_on = true;
        Some(self.draw_caret(c))
    }
}
