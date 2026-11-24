//! The First Light shell: one calm terminal window on the desktop.

use alloc::string::{String, ToString};
use alloc::vec::Vec;
use alloc::format;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};
use x86_64::instructions::port::Port;

use crate::boot_screen::ease_in_out;
use crate::desktop::Desktop;
use crate::keyboard::{Key, KeyCode};
use crate::screen::{Rgb, Screen};
use crate::theme::*;
use crate::{heap, memory, time, SCREEN, VERSION};

const FONT: RasterHeight = RasterHeight::Size16;
const LINE_H: usize = 22;
const PROMPT: &str = "~ ";
const MAX_LINES: usize = 500;
const BLINK_MS: u64 = 530;

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

pub struct Shell {
    desktop: Desktop,
    lines: Vec<(String, Kind)>,
    input: String,
    history: Vec<String>,
    history_pos: usize,
    // Text area geometry, in pixels.
    x: usize,
    top: usize,
    width: usize,
    rows: usize,
    cols: usize,
    cursor_on: bool,
    last_blink: u64,
    last_minute: u8,
    started: u64,
}

impl Shell {
    pub fn new(s: &Screen, boot_ms: u64) -> Self {
        let desktop = Desktop::new(s);
        let r = desktop.window;
        let x = r.x + 28;
        let top = r.y + 64;
        let width = r.w - 56;
        let rows = (r.y + r.h - 20 - top) / LINE_H;
        let cols = width / Screen::glyph_width(FONT, FontWeight::Regular);
        let mut sh = Self {
            desktop,
            lines: Vec::new(),
            input: String::new(),
            history: Vec::new(),
            history_pos: 0,
            x,
            top,
            width,
            rows,
            cols,
            cursor_on: true,
            last_blink: 0,
            last_minute: 255,
            started: boot_ms,
        };
        sh.say(&format!("Dante OS {VERSION}. Good morning."), Kind::Input);
        sh.say("Type 'help' to see what I can do.", Kind::Output);
        sh.say("", Kind::Output);
        sh
    }

    /// Paints the whole desktop, with the window rising gently into place.
    pub fn show(&mut self) {
        {
            let mut g = SCREEN.lock();
            let s = g.as_mut().unwrap();
            s.clear();
            self.desktop.draw_wallpaper(s);
            self.desktop.draw_header(s);
            self.desktop.draw_footer(s, "type help  ·  esc clears the line");
        }
        let start = time::millis();
        const RISE: u64 = 600;
        loop {
            let t = ((time::millis() - start) as f32 / RISE as f32).min(1.0);
            {
                let mut g = SCREEN.lock();
                let s = g.as_mut().unwrap();
                self.desktop.draw_window(s, "01 - terminal   ~", ease_in_out(t));
            }
            if t >= 1.0 {
                break;
            }
            time::sleep_ms(16);
        }
        self.last_minute = time::now().minute;
        let mut g = SCREEN.lock();
        let s = g.as_mut().unwrap();
        let r = self.desktop.window;
        s.rect(r.x + 1, r.y + 50, r.w - 2, 1, HAIRLINE);
        self.redraw(s);
    }

    /// Runs forever: keys, the cursor blink and the clock.
    pub fn run(&mut self) -> ! {
        loop {
            while let Some(key) = crate::keyboard::next_key() {
                self.key(key);
            }
            let now = time::millis();
            if now - self.last_blink >= BLINK_MS {
                self.last_blink = now;
                self.cursor_on = !self.cursor_on;
                self.with_screen(|sh, s| sh.draw_input(s));
                let minute = time::now().minute;
                if minute != self.last_minute {
                    self.last_minute = minute;
                    self.with_screen(|sh, s| sh.desktop.draw_header(s));
                }
            }
            x86_64::instructions::hlt();
        }
    }

    fn with_screen(&mut self, f: impl FnOnce(&mut Self, &mut Screen)) {
        let mut g = SCREEN.lock();
        if let Some(s) = g.as_mut() {
            f(self, s);
        }
    }

    fn key(&mut self, key: Key) {
        self.cursor_on = true;
        self.last_blink = time::millis();
        match key {
            Key::Char('\n') => {
                let line = core::mem::take(&mut self.input);
                self.say(&format!("{PROMPT}{line}"), Kind::Input);
                let cmd = line.trim().to_string();
                if !cmd.is_empty() {
                    if self.history.last() != Some(&cmd) {
                        self.history.push(cmd.clone());
                    }
                    self.run_command(&cmd);
                }
                self.history_pos = self.history.len();
                self.with_screen(|sh, s| sh.redraw(s));
                return;
            }
            Key::Char('\u{8}') => {
                self.input.pop();
            }
            Key::Char('\u{1b}') => self.input.clear(),
            Key::Char('\t') => {}
            Key::Char(c) if !c.is_control() => {
                if PROMPT.len() + self.input.chars().count() + 1 < self.cols {
                    self.input.push(c);
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
            _ => return,
        }
        self.with_screen(|sh, s| sh.draw_input(s));
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

    fn visible(&self) -> &[(String, Kind)] {
        let keep = self.rows.saturating_sub(1);
        &self.lines[self.lines.len().saturating_sub(keep)..]
    }

    fn clear_row(&self, s: &mut Screen, row: usize) {
        let y0 = self.top + row * LINE_H;
        for y in y0..y0 + LINE_H {
            let c = self.desktop.paper_at(s, y);
            s.rect(self.x - 4, y, self.width + 8, 1, c);
        }
    }

    fn redraw(&self, s: &mut Screen) {
        let visible = self.visible();
        for row in 0..self.rows {
            self.clear_row(s, row);
            if let Some((text, kind)) = visible.get(row) {
                s.text(self.x, self.top + row * LINE_H + 3, text, FONT, FontWeight::Regular, kind.colour(), 0, 1.0);
            }
        }
        self.draw_input(s);
    }

    fn draw_input(&self, s: &mut Screen) {
        let row = self.visible().len();
        self.clear_row(s, row);
        let y = self.top + row * LINE_H + 3;
        let end = s.text(self.x, y, PROMPT, FONT, FontWeight::Regular, SEA, 0, 1.0);
        let end = s.text(end, y, &self.input, FONT, FontWeight::Regular, INK, 0, 1.0);
        if self.cursor_on {
            s.rect(end + 1, y, 2, 17, SEA);
        }
    }

    // ---- Commands ----

    fn run_command(&mut self, line: &str) {
        let (cmd, rest) = line.split_once(' ').map(|(c, r)| (c, r.trim())).unwrap_or((line, ""));
        match cmd {
            "help" => {
                for (name, what) in [
                    ("help", "this list"),
                    ("about", "what Dante OS is"),
                    ("mem", "memory in use"),
                    ("cpu", "the processor we're running on"),
                    ("uptime", "time since boot"),
                    ("time", "today's date and time"),
                    ("echo <text>", "say something back"),
                    ("clear", "a clean page"),
                    ("crash", "test the crash screen"),
                    ("reboot", "start again"),
                    ("shutdown", "power off"),
                ] {
                    self.say(&format!("  {name:<14}{what}"), Kind::Output);
                }
            }
            "about" | "version" => {
                self.say(&format!("Dante OS {VERSION}, First Light."), Kind::Accent);
                self.say("A small kernel in Rust, written from scratch.", Kind::Output);
                self.say("x86-64 · Limine boot · APIC timer at 1000 Hz · PS/2 keyboard", Kind::Output);
            }
            "mem" => {
                let (heap_used, heap_free) = heap::stats();
                let (used, total) = memory::stats();
                self.say(&format!("  ram    {} MiB of {} MiB in use", used * 4 / 1024, total * 4 / 1024), Kind::Output);
                self.say(
                    &format!("  heap   {} KiB used, {} KiB free", heap_used / 1024, heap_free / 1024),
                    Kind::Output,
                );
            }
            "cpu" => {
                let (vendor, brand) = cpu_names();
                self.say(&format!("  {}", brand.trim()), Kind::Output);
                self.say(&format!("  vendor {vendor} · local APIC {}", crate::apic::lapic_id()), Kind::Output);
            }
            "uptime" => {
                let ms = time::millis() - self.started;
                let secs = ms / 1000;
                self.say(&format!("  up {}h {:02}m {:02}s", secs / 3600, secs / 60 % 60, secs % 60), Kind::Output);
            }
            "time" | "date" => {
                let n = time::now();
                const MONTHS: [&str; 12] =
                    ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
                let month = MONTHS.get(n.month.saturating_sub(1) as usize).unwrap_or(&"?");
                self.say(
                    &format!("  {} {month} {}  {:02}:{:02}:{:02} UTC", n.day, n.year, n.hour, n.minute, n.second),
                    Kind::Output,
                );
            }
            "echo" => self.say(rest, Kind::Output),
            "clear" => self.lines.clear(),
            "crash" => {
                // Deliberately touch unmapped memory to show the crash screen.
                unsafe { core::ptr::read_volatile(0x4000_dead_0000 as *const u64) };
            }
            "reboot" => {
                self.say("  restarting...", Kind::Output);
                self.with_screen(|sh, s| sh.redraw(s));
                time::sleep_ms(400);
                unsafe { Port::<u8>::new(0x64).write(0xFE) };
            }
            "shutdown" | "poweroff" => {
                self.say("  goodbye.", Kind::Output);
                self.with_screen(|sh, s| sh.redraw(s));
                time::sleep_ms(400);
                // QEMU and Bochs power-off ports (real ACPI shutdown comes later).
                unsafe {
                    Port::<u16>::new(0x604).write(0x2000);
                    Port::<u16>::new(0xB004).write(0x2000);
                }
                self.say("  this machine can't power off yet.", Kind::Error);
            }
            _ => self.say(&format!("  I don't know '{cmd}' yet. Try 'help'."), Kind::Error),
        }
    }
}

fn cpu_names() -> (String, String) {
    use core::arch::x86_64::__cpuid;
    let regs = |leaf: u32| __cpuid(leaf);
    let r = regs(0);
    let mut vendor = Vec::new();
    for v in [r.ebx, r.edx, r.ecx] {
        vendor.extend_from_slice(&v.to_le_bytes());
    }
    let mut brand = Vec::new();
    if regs(0x8000_0000).eax >= 0x8000_0004 {
        for leaf in 0x8000_0002..=0x8000_0004 {
            let r = regs(leaf);
            for v in [r.eax, r.ebx, r.ecx, r.edx] {
                brand.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    let text = |b: Vec<u8>| String::from_utf8_lossy(&b).trim_end_matches('\0').to_string();
    (text(vendor), text(brand))
}
