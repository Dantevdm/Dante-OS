//! The launcher: one big quiet search line over the desktop. Type to find an
//! app or a command, Enter to open it, 1–9 to jump, ">" to run a shell command.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::apps::AppKind;
use crate::boot_screen::ease_out;
use crate::desktop::MARGIN;
use crate::gfx::{Canvas, Rect, Rgb, Surface, text_width};
use crate::keyboard::{Key, KeyCode};
use crate::theme::*;

const BG: Rgb = Rgb::hex(0xEEEFEC);
const ROW_TEXT: Rgb = Rgb::hex(0x46545A);
const ROW_LINE: Rgb = Rgb::hex(0xD3DADA);
const PLACEHOLDER: Rgb = Rgb::hex(0xB4BFC1);
const FADE_MS: u64 = 220;
const ROW_H: i32 = 62;
const BIG: f32 = 2.4;

#[derive(Clone)]
pub enum Action {
    Open(AppKind),
    Run(String),
    Reboot,
    Shutdown,
}

struct Item {
    name: String,
    kind: &'static str,
    action: Action,
}

pub enum Outcome {
    Nothing,
    Redraw,
    Close,
    Activate(Action),
}

pub struct Launcher {
    pub canvas: Canvas,
    query: String,
    items: Vec<Item>,
    sel: usize,
    opened: u64,
    w: i32,
    h: i32,
}

impl Launcher {
    pub fn new(w: usize, h: usize, query: &str, now: u64) -> Self {
        let mut l = Self {
            canvas: Canvas::new(w, h, 0),
            query: query.to_string(),
            items: Vec::new(),
            sel: 0,
            opened: now,
            w: w as i32,
            h: h as i32,
        };
        l.refresh();
        l
    }

    fn sy(&self, v: f32) -> i32 {
        (v * self.h as f32 / 900.0) as i32
    }

    fn rows_top(&self) -> i32 {
        self.sy(300.0) + 30
    }

    fn max_rows(&self) -> usize {
        ((self.h - 80 - self.rows_top()) / ROW_H).clamp(1, 9) as usize
    }

    fn refresh(&mut self) {
        let q = self.query.trim().to_lowercase();
        self.items.clear();
        if let Some(cmd) = self.query.strip_prefix('>') {
            let cmd = cmd.trim();
            if !cmd.is_empty() {
                self.items.push(Item { name: cmd.to_string(), kind: "shell command", action: Action::Run(cmd.to_string()) });
            }
        } else {
            let mut all: Vec<Item> = AppKind::ALL
                .iter()
                .map(|&k| {
                    let mut name = k.name().to_string();
                    name[..1].make_ascii_uppercase();
                    Item { name, kind: "app", action: Action::Open(k) }
                })
                .collect();
            all.push(Item { name: "Restart".into(), kind: "command", action: Action::Reboot });
            all.push(Item { name: "Shut down".into(), kind: "command", action: Action::Shutdown });
            for (cmd, what) in [("mem", "Memory in use"), ("cpu", "Processor"), ("uptime", "Uptime")] {
                all.push(Item { name: what.into(), kind: "command", action: Action::Run(cmd.into()) });
            }
            self.items = all.into_iter().filter(|i| q.is_empty() || i.name.to_lowercase().contains(&q) || i.kind.contains(&q)).collect();
        }
        self.items.truncate(self.max_rows());
        self.sel = self.sel.min(self.items.len().saturating_sub(1));
        self.paint();
    }

    pub fn alpha(&self, now: u64) -> u32 {
        (ease_out(((now - self.opened) as f32 / FADE_MS as f32).min(1.0)) * 255.0) as u32
    }

    pub fn animating(&self, now: u64) -> bool {
        now - self.opened <= FADE_MS + 20
    }

    pub fn key(&mut self, key: Key) -> Outcome {
        match key {
            Key::Char('\u{1b}') => return Outcome::Close,
            Key::Char('\n') => return self.action(self.sel).map_or(Outcome::Nothing, Outcome::Activate),
            Key::Char(d @ '1'..='9') if self.query.is_empty() => {
                return self.action(d as usize - '1' as usize).map_or(Outcome::Nothing, Outcome::Activate);
            }
            Key::Char('\u{8}') => {
                if self.query.pop().is_none() {
                    return Outcome::Nothing;
                }
            }
            Key::Char(c) if !c.is_control() => {
                if self.query.chars().count() < 40 {
                    self.query.push(c);
                }
            }
            Key::Raw(KeyCode::ArrowDown) if self.sel + 1 < self.items.len() => self.sel += 1,
            Key::Raw(KeyCode::ArrowUp) if self.sel > 0 => self.sel -= 1,
            _ => return Outcome::Nothing,
        }
        self.refresh();
        Outcome::Redraw
    }

    pub fn action(&self, i: usize) -> Option<Action> {
        self.items.get(i).map(|i| i.action.clone())
    }

    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        if x < MARGIN || x >= self.w - MARGIN || y < self.rows_top() {
            return None;
        }
        let i = ((y - self.rows_top()) / ROW_H) as usize;
        (i < self.items.len()).then_some(i)
    }

    /// Select the row under the pointer. True if that changed anything.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        match self.row_at(x, y) {
            Some(i) if i != self.sel => {
                self.sel = i;
                self.paint();
                true
            }
            _ => false,
        }
    }

    fn paint(&mut self) {
        let (w, h) = (self.w, self.h);
        let c = &mut self.canvas;
        let all = c.bounds();
        c.fill(all, BG.argb(252));
        let f = RasterHeight::Size16;

        c.text(MARGIN as usize, 26, "DANTE", f, FontWeight::Regular, INK, 2, 1.0);
        let esc = "esc to close";
        let ew = text_width(esc, f, FontWeight::Regular, 1) as i32;
        c.text((w - MARGIN - ew) as usize, 26, esc, f, FontWeight::Regular, MUTED, 1, 1.0);

        // The search line, in big light type.
        let iy = (150.0 * h as f32 / 900.0) as f32 - 10.0;
        let caret_h = (32.0 * BIG) as i32 - 14;
        let x = MARGIN as f32;
        if self.query.is_empty() {
            c.fill_rgb(Rect::new(MARGIN, iy as i32 + 8, 3, caret_h), SEA);
            c.text_scaled(x + 14.0, iy, "type anything", RasterHeight::Size32, FontWeight::Light, PLACEHOLDER, BIG, -4.0, 1.0);
        } else {
            let end = c.text_scaled(x, iy, &self.query, RasterHeight::Size32, FontWeight::Light, INK, BIG, -4.0, 1.0);
            c.fill_rgb(Rect::new(end as i32 + 6, iy as i32 + 8, 3, caret_h), SEA);
        }

        // A ruler under it.
        let ly = (300.0 * h as f32 / 900.0) as i32;
        c.fill_rgb(Rect::new(MARGIN, ly, w - 2 * MARGIN, 1), INK);
        for i in 0..=4 {
            let tx = MARGIN + (w - 2 * MARGIN - 1) * i / 4;
            let len = if i % 2 == 0 { 10 } else { 6 };
            c.fill_rgb(Rect::new(tx, ly, 1, len), RULE);
        }

        // Results.
        let top = ly + 30;
        if self.items.is_empty() {
            let msg = if self.query.starts_with('>') {
                String::from("type a command for the terminal")
            } else {
                format!("nothing called '{}' yet · start with > to run it as a command", self.query.trim())
            };
            c.text(MARGIN as usize, (top + 22) as usize, &msg, f, FontWeight::Regular, MUTED, 0, 1.0);
        }
        for (i, item) in self.items.iter().enumerate() {
            let ry = top + i as i32 * ROW_H;
            let sel = i == self.sel;
            c.text(MARGIN as usize, (ry + 23) as usize, &format!("{:02}", i + 1), f, FontWeight::Regular, MUTED, 0, 1.0);
            let (weight, colour) = if sel { (FontWeight::Regular, INK) } else { (FontWeight::Light, ROW_TEXT) };
            c.text((MARGIN + 64) as usize, (ry + 14) as usize, &item.name, RasterHeight::Size32, weight, colour, 0, 1.0);
            let kw = text_width(item.kind, f, FontWeight::Regular, 0) as i32;
            c.text((w - MARGIN - 34 - kw) as usize, (ry + 23) as usize, item.kind, f, FontWeight::Regular, MUTED, 0, 1.0);
            if sel {
                c.disc((w - MARGIN - 6) as f32, (ry + 31) as f32, 4.5, SEA);
            }
            c.fill_rgb(Rect::new(MARGIN, ry + ROW_H - 1, w - 2 * MARGIN, 1), ROW_LINE);
        }

        // Hints.
        let mut hx = MARGIN as usize;
        let hy = (h - 52) as usize;
        for (k, what) in [("enter", " open"), ("1-9", " jump"), (">", " shell command"), ("ctrl space", " from anywhere")] {
            hx = c.text(hx, hy, k, f, FontWeight::Regular, INK, 0, 1.0);
            hx = c.text(hx, hy, what, f, FontWeight::Regular, MUTED, 0, 1.0) + 36;
        }
    }
}
