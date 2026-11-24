//! The terminal: commands for the kernel.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use super::{App, AppKind, BLINK_MS, Request};
use crate::gfx::{Canvas, Rect, Rgb, Surface, glyph_width};
use crate::keyboard::{Key, KeyCode};
use crate::theme::*;
use crate::{heap, memory, time, VERSION};

const FONT: RasterHeight = RasterHeight::Size16;
const LINE_H: i32 = 22;
const PROMPT: &str = "~ ";
const MAX_LINES: usize = 500;

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Input,
    Output,
    Accent,
    Error,
}

impl Kind {
    fn colour(self) -> Rgb {
        match self {
            Kind::Input => INK,
            Kind::Output => MUTED,
            Kind::Accent => SEA,
            Kind::Error => TERRACOTTA,
        }
    }
}

pub struct Terminal {
    lines: Vec<(String, Kind)>,
    input: String,
    history: Vec<String>,
    history_pos: usize,
    cols: usize,
    caret_on: bool,
    focused: bool,
    last_blink: u64,
}

impl Terminal {
    pub fn new() -> Self {
        let mut t = Self {
            lines: Vec::new(),
            input: String::new(),
            history: Vec::new(),
            history_pos: 0,
            cols: 72,
            caret_on: true,
            focused: true,
            last_blink: 0,
        };
        t.say(&format!("Dante OS {VERSION}. Good morning."), Kind::Input);
        t.say("Type 'help' to see what I can do.", Kind::Output);
        t.say("", Kind::Output);
        t
    }

    fn rows(area: Rect) -> usize {
        (area.h / LINE_H) as usize
    }

    fn row_rect(area: Rect, row: usize) -> Rect {
        Rect::new(area.x, area.y + row as i32 * LINE_H, area.w, LINE_H)
    }

    /// Adds output, wrapping long lines to the window width.
    fn say(&mut self, text: &str, kind: Kind) {
        for raw in text.split('\n') {
            let chars: Vec<char> = raw.chars().collect();
            if chars.is_empty() {
                self.lines.push((String::new(), kind));
            }
            for chunk in chars.chunks(self.cols.max(1)) {
                self.lines.push((chunk.iter().collect(), kind));
            }
        }
        if self.lines.len() > MAX_LINES {
            let extra = self.lines.len() - MAX_LINES;
            self.lines.drain(..extra);
        }
    }

    fn visible(&self, area: Rect) -> &[(String, Kind)] {
        let keep = Self::rows(area).saturating_sub(1);
        &self.lines[self.lines.len().saturating_sub(keep)..]
    }

    fn draw_input(&self, c: &mut Canvas, area: Rect) -> Rect {
        let r = Self::row_rect(area, self.visible(area).len());
        c.fill_rgb(r, PAPER);
        let y = (r.y + 3) as usize;
        let end = c.text(r.x as usize, y, PROMPT, FONT, FontWeight::Regular, SEA, 0, 1.0);
        let end = c.text(end, y, &self.input, FONT, FontWeight::Regular, INK, 0, 1.0);
        if self.caret_on && self.focused {
            c.fill_rgb(Rect::new(end as i32 + 1, y as i32, 2, 17), SEA);
        }
        r
    }

    fn exec(&mut self, line: &str, out: &mut Vec<Request>) {
        let (cmd, rest) = line.split_once(' ').map(|(c, r)| (c, r.trim())).unwrap_or((line, ""));
        match cmd {
            "help" => {
                for (name, what) in [
                    ("open <app>", "open terminal, notes, clock or system"),
                    ("about", "what Dante OS is"),
                    ("mem", "memory in use"),
                    ("cpu", "the processor we're running on"),
                    ("uptime", "time since boot"),
                    ("time", "today's date and time"),
                    ("echo <text>", "say something back"),
                    ("clear", "a clean page"),
                    ("close", "close this window"),
                    ("crash", "test the crash screen"),
                    ("reboot", "start again"),
                    ("shutdown", "power off"),
                ] {
                    self.say(&format!("  {name:<13}{what}"), Kind::Output);
                }
            }
            "about" | "version" => {
                self.say(&format!("Dante OS {VERSION}, First Light."), Kind::Accent);
                self.say("A small kernel in Rust, written from scratch.", Kind::Output);
                self.say("x86-64 · APIC timer · PS/2 keyboard and mouse · windows", Kind::Output);
            }
            "apps" => {
                for k in AppKind::ALL {
                    self.say(&format!("  {:<13}{}", k.name(), k.describe()), Kind::Output);
                }
            }
            "open" => match AppKind::from_name(rest) {
                Some(k) => out.push(Request::Open(k)),
                None => self.say("  open what? terminal, notes, clock or system", Kind::Error),
            },
            "notes" | "clock" | "system" => out.push(Request::Open(AppKind::from_name(cmd).unwrap())),
            "mem" => {
                let (heap_used, heap_free) = heap::stats();
                let (used, total) = memory::stats();
                self.say(&format!("  ram    {} MiB of {} MiB in use", used * 4 / 1024, total * 4 / 1024), Kind::Output);
                self.say(&format!("  heap   {} KiB used, {} KiB free", heap_used / 1024, heap_free / 1024), Kind::Output);
            }
            "cpu" => {
                let (vendor, brand) = crate::cpu::names();
                self.say(&format!("  {brand}"), Kind::Output);
                self.say(&format!("  vendor {vendor} · {} cores", super::cores()), Kind::Output);
            }
            "uptime" => self.say(&format!("  up {}", super::uptime_text()), Kind::Output),
            "time" | "date" => {
                let n = time::now();
                self.say(&format!("  {}  {:02}:{:02}:{:02}", n.date_text(), n.hour, n.minute, n.second), Kind::Output);
            }
            "echo" => self.say(rest, Kind::Output),
            "clear" => self.lines.clear(),
            "close" | "exit" => out.push(Request::Close),
            "crash" => {
                // Deliberately touch memory that isn't mapped, to show the crash screen.
                unsafe { core::ptr::read_volatile(0x4000_dead_0000 as *const u64) };
            }
            "reboot" | "restart" => out.push(Request::Reboot),
            "shutdown" | "poweroff" => out.push(Request::Shutdown),
            _ => self.say(&format!("  I don't know '{cmd}' yet. Try 'help'."), Kind::Error),
        }
    }
}

impl App for Terminal {
    fn kind(&self) -> AppKind {
        AppKind::Terminal
    }

    fn size(&self) -> (i32, i32) {
        (660, 430)
    }

    fn subtitle(&self) -> &str {
        "~"
    }

    fn draw(&mut self, c: &mut Canvas, area: Rect) {
        self.cols = (area.w as usize / glyph_width(FONT, FontWeight::Regular)).max(10);
        c.fill_rgb(area, PAPER);
        for (row, (text, kind)) in self.visible(area).iter().enumerate() {
            let r = Self::row_rect(area, row);
            c.text(r.x as usize, (r.y + 3) as usize, text, FONT, FontWeight::Regular, kind.colour(), 0, 1.0);
        }
        self.draw_input(c, area);
    }

    fn key(&mut self, key: Key, c: &mut Canvas, area: Rect, out: &mut Vec<Request>) -> Option<Rect> {
        self.caret_on = true;
        self.last_blink = time::millis();
        match key {
            Key::Char('\n') => {
                let line = core::mem::take(&mut self.input);
                return self.run(line.trim(), c, area, out);
            }
            Key::Char('\u{8}') => {
                self.input.pop();
            }
            Key::Char('\u{1b}') => self.input.clear(),
            Key::Char(ch) if !ch.is_control() => {
                if PROMPT.len() + self.input.chars().count() + 1 < self.cols {
                    self.input.push(ch);
                }
            }
            Key::Raw(KeyCode::ArrowUp) if self.history_pos > 0 => {
                self.history_pos -= 1;
                self.input = self.history[self.history_pos].clone();
            }
            Key::Raw(KeyCode::ArrowDown) if self.history_pos < self.history.len() => {
                self.history_pos += 1;
                self.input = self.history.get(self.history_pos).cloned().unwrap_or_default();
            }
            _ => return None,
        }
        Some(self.draw_input(c, area))
    }

    fn run(&mut self, cmd: &str, c: &mut Canvas, area: Rect, out: &mut Vec<Request>) -> Option<Rect> {
        self.say(&format!("{PROMPT}{cmd}"), Kind::Input);
        let cmd = cmd.to_string();
        if !cmd.is_empty() {
            if self.history.last() != Some(&cmd) {
                self.history.push(cmd.clone());
            }
            self.exec(&cmd, out);
        }
        self.history_pos = self.history.len();
        self.draw(c, area);
        Some(area)
    }

    fn tick(&mut self, now: u64, c: &mut Canvas, area: Rect) -> Option<Rect> {
        if !self.focused || now - self.last_blink < BLINK_MS {
            return None;
        }
        self.last_blink = now;
        self.caret_on = !self.caret_on;
        Some(self.draw_input(c, area))
    }

    fn focus(&mut self, focused: bool, c: &mut Canvas, area: Rect) -> Option<Rect> {
        self.focused = focused;
        self.caret_on = true;
        Some(self.draw_input(c, area))
    }
}
