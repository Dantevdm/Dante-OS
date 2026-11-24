//! Files: browse the disk. Double-click or Enter opens, Backspace goes up a
//! folder, Delete removes the selected item.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use super::{App, AppKind, Request};
use crate::fs::{self, Entry};
use crate::gfx::{Canvas, Rect, Rgb, Surface, glyph_width, text_width};
use crate::keyboard::{Key, KeyCode};
use crate::theme::*;
use crate::time;

const FONT: RasterHeight = RasterHeight::Size16;
const ROW_H: i32 = 34;
const HEADER_H: i32 = 42;
const DOUBLE_CLICK_MS: u64 = 450;
const REFRESH_MS: u64 = 1500;
const SELECTED: Rgb = Rgb::hex(0xE4EBEC);
const DIVIDER: Rgb = Rgb::hex(0xE2E7E6);
const FOLDER: Rgb = Rgb::hex(0xCFDDE0);

pub struct Files {
    cwd: String,
    entries: Vec<Entry>,
    sel: Option<usize>,
    scroll: usize,
    last_click: (u64, usize),
    last_refresh: u64,
}

impl Files {
    pub fn new(path: &str) -> Self {
        let mut f = Self { cwd: String::from(path), entries: Vec::new(), sel: None, scroll: 0, last_click: (0, usize::MAX), last_refresh: 0 };
        f.reload();
        f
    }

    fn reload(&mut self) -> bool {
        let next = fs::list(&self.cwd).unwrap_or_default();
        let same = next.len() == self.entries.len()
            && next.iter().zip(&self.entries).all(|(a, b)| a.name == b.name && a.size == b.size && a.modified == b.modified);
        self.entries = next;
        if let Some(s) = self.sel {
            if s >= self.entries.len() {
                self.sel = self.entries.len().checked_sub(1);
            }
        }
        !same
    }

    fn path_of(&self, name: &str) -> String {
        fs::resolve(&self.cwd, name)
    }

    fn go(&mut self, path: String) {
        self.cwd = path;
        self.sel = None;
        self.scroll = 0;
        self.reload();
    }

    fn rows(area: Rect) -> usize {
        ((area.h - HEADER_H) / ROW_H).max(1) as usize
    }

    fn keep_visible(&mut self, area: Rect) {
        let rows = Self::rows(area);
        if let Some(s) = self.sel {
            if s < self.scroll {
                self.scroll = s;
            } else if s >= self.scroll + rows {
                self.scroll = s + 1 - rows;
            }
        }
    }

    fn back_rect(area: Rect) -> Rect {
        Rect::new(area.x - 6, area.y - 4, 26, 28)
    }

    fn open(&mut self, i: usize, out: &mut Vec<Request>) -> bool {
        let Some(e) = self.entries.get(i) else { return false };
        let path = self.path_of(&e.name);
        if e.dir {
            self.go(path);
            true
        } else {
            out.push(Request::OpenFile(path));
            false
        }
    }
}

fn draw_folder(c: &mut Canvas, x: i32, y: i32) {
    c.panel(Rect::new(x, y + 1, 8, 5), 2.0, FOLDER, 1.0, Some(RULE), 0.0);
    c.panel(Rect::new(x, y + 3, 17, 12), 2.5, FOLDER, 1.0, Some(RULE), 0.0);
}

fn draw_file(c: &mut Canvas, x: i32, y: i32) {
    c.panel(Rect::new(x + 2, y, 13, 16), 2.0, PAPER, 1.0, Some(RULE), 0.0);
    for k in 0..3 {
        c.fill_rgb(Rect::new(x + 5, y + 5 + k * 3, 7, 1), HAIRLINE);
    }
}

impl App for Files {
    fn kind(&self) -> AppKind {
        AppKind::Files
    }

    fn size(&self) -> (i32, i32) {
        (580, 460)
    }

    fn draw(&mut self, c: &mut Canvas, area: Rect) {
        c.fill_rgb(area, PAPER);
        self.keep_visible(area);

        // Where we are, with a way back up.
        let mut px = area.x;
        if self.cwd != "/" {
            let (bx, by) = (area.x as f32 + 6.0, area.y as f32 + 10.0);
            c.stroke(bx + 4.0, by - 5.0, bx - 1.0, by, INK);
            c.stroke(bx - 1.0, by, bx + 4.0, by + 5.0, INK);
            px += 26;
        }
        c.text(px as usize, (area.y + 2) as usize, &self.cwd, FONT, FontWeight::Bold, INK, 0, 1.0);
        c.fill_rgb(Rect::new(area.x, area.y + HEADER_H - 10, area.w, 1), HAIRLINE);

        if self.entries.is_empty() {
            c.text(area.x as usize, (area.y + HEADER_H + 8) as usize, "Empty", FONT, FontWeight::Regular, MUTED, 0, 1.0);
            return;
        }

        let gw = glyph_width(FONT, FontWeight::Regular) as i32;
        let date_w = 13 * gw;
        let size_right = area.right() - date_w - 24;
        let name_max = ((size_right - 90 - (area.x + 28)) / gw).max(4) as usize;
        for (row, i) in (self.scroll..self.entries.len()).take(Self::rows(area)).enumerate() {
            let e = &self.entries[i];
            let y = area.y + HEADER_H + row as i32 * ROW_H;
            if self.sel == Some(i) {
                c.panel(Rect::new(area.x - 8, y, area.w + 16, ROW_H - 2), 6.0, SELECTED, 1.0, None, 0.0);
            } else {
                c.fill_rgb(Rect::new(area.x, y + ROW_H - 1, area.w, 1), DIVIDER);
            }
            if e.dir {
                draw_folder(c, area.x, y + 8);
            } else {
                draw_file(c, area.x, y + 8);
            }
            let name: String = e.name.chars().take(name_max).collect();
            let ty = (y + 8) as usize;
            c.text((area.x + 28) as usize, ty, &name, FONT, FontWeight::Regular, INK, 0, 1.0);
            if !e.dir {
                let size = fs::size_text(e.size as u64);
                let sw = text_width(&size, FONT, FontWeight::Regular, 0) as i32;
                c.text((size_right - sw) as usize, ty, &size, FONT, FontWeight::Regular, MUTED, 0, 1.0);
            }
            let when = time::DateTime::from_timestamp(e.modified).short_text();
            let ww = text_width(&when, FONT, FontWeight::Regular, 0) as i32;
            c.text((area.right() - ww) as usize, ty, &when, FONT, FontWeight::Regular, MUTED, 0, 1.0);
        }
    }

    fn key(&mut self, key: Key, c: &mut Canvas, area: Rect, out: &mut Vec<Request>) -> Option<Rect> {
        let n = self.entries.len();
        match key {
            Key::Raw(KeyCode::ArrowDown) if n > 0 => self.sel = Some(self.sel.map_or(0, |s| (s + 1).min(n - 1))),
            Key::Raw(KeyCode::ArrowUp) if n > 0 => self.sel = Some(self.sel.map_or(0, |s| s.saturating_sub(1))),
            Key::Char('\n') => {
                let Some(i) = self.sel else { return None };
                if !self.open(i, out) {
                    return None;
                }
            }
            Key::Char('\u{8}') if self.cwd != "/" => {
                let up = fs::resolve(&self.cwd, "..");
                self.go(up);
            }
            Key::Char('\u{7f}') | Key::Raw(KeyCode::Delete) => {
                let Some(i) = self.sel else { return None };
                let path = self.path_of(&self.entries[i].name);
                if let Err(e) = fs::remove(&path) {
                    crate::log!("files: could not remove {path}: {}", e.text());
                    return None;
                }
                self.reload();
            }
            _ => return None,
        }
        self.draw(c, area);
        Some(area)
    }

    fn click(&mut self, x: i32, y: i32, c: &mut Canvas, area: Rect, out: &mut Vec<Request>) -> Option<Rect> {
        if self.cwd != "/" && Self::back_rect(area).contains(x, y) {
            let up = fs::resolve(&self.cwd, "..");
            self.go(up);
            self.draw(c, area);
            return Some(area);
        }
        let top = area.y + HEADER_H;
        if y < top || x < area.x - 8 || x > area.right() + 8 {
            return None;
        }
        let i = self.scroll + ((y - top) / ROW_H) as usize;
        if i >= self.entries.len() {
            self.sel = None;
        } else {
            let now = time::millis();
            let double = self.last_click.1 == i && now - self.last_click.0 < DOUBLE_CLICK_MS;
            self.last_click = (now, i);
            self.sel = Some(i);
            if double {
                self.last_click = (0, usize::MAX);
                self.open(i, out);
            }
        }
        self.draw(c, area);
        Some(area)
    }

    fn tick(&mut self, now: u64, c: &mut Canvas, area: Rect) -> Option<Rect> {
        if now - self.last_refresh < REFRESH_MS {
            return None;
        }
        self.last_refresh = now;
        if !self.reload() {
            return None;
        }
        self.draw(c, area);
        Some(area)
    }

    fn title(&self) -> String {
        match self.cwd.rsplit('/').next() {
            Some(name) if !name.is_empty() => String::from(name),
            _ => format!("Disk"),
        }
    }
}
