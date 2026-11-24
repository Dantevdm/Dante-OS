//! The terminal: commands for the kernel.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use super::{App, AppKind, BLINK_MS, Request};
use crate::gfx::{Canvas, Rect, Rgb, Surface, glyph_width};
use crate::keyboard::{Key, KeyCode};
use crate::theme::*;
use crate::process::{self, Status};
use crate::{fs, heap, keyboard, memory, programs, time, VERSION};

const FONT: RasterHeight = RasterHeight::Size16;
const LINE_H: i32 = 22;
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
    cwd: String,
    cols: usize,
    caret_on: bool,
    focused: bool,
    last_blink: u64,
    /// The program running in the foreground: (pid, name).
    job: Option<(u32, String)>,
    /// Program output that hasn't ended its line yet.
    partial: String,
}

impl Terminal {
    pub fn new() -> Self {
        let mut t = Self {
            lines: Vec::new(),
            input: String::new(),
            history: Vec::new(),
            history_pos: 0,
            cwd: String::from("/"),
            cols: 72,
            caret_on: true,
            focused: true,
            last_blink: 0,
            job: None,
            partial: String::new(),
        };
        t.say(&format!("Dante OS {VERSION}"), Kind::Output);
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
        // While a program runs, its unfinished line (often a question) comes first.
        let (lead, colour) = if self.job.is_some() { (self.partial.clone(), MUTED) } else { (self.prompt(), SEA) };
        let end = c.text(r.x as usize, y, &lead, FONT, FontWeight::Regular, colour, 0, 1.0);
        let end = c.text(end, y, &self.input, FONT, FontWeight::Regular, INK, 0, 1.0);
        if self.caret_on && self.focused {
            c.fill_rgb(Rect::new(end as i32 + 1, y as i32, 2, 17), SEA);
        }
        r
    }

    fn prompt(&self) -> String {
        format!("{} $ ", self.cwd)
    }

    fn path(&self, p: &str) -> String {
        fs::resolve(&self.cwd, p)
    }

    /// The program a command names: a name in /Programs, or a path.
    fn program(&self, cmd: &str) -> Option<String> {
        let path = if cmd.contains('/') { self.path(cmd) } else { format!("{}/{cmd}", programs::DIR) };
        matches!(fs::stat(&path), Ok(e) if !e.dir).then_some(path)
    }

    /// The start of the input line: the prompt, or a program's unfinished line.
    fn lead(&self) -> String {
        if self.job.is_some() { self.partial.clone() } else { self.prompt() }
    }

    /// Collect output from the running program. True if anything changed.
    fn pump(&mut self) -> bool {
        let Some((pid, name)) = self.job.clone() else { return false };
        let Some((output, status)) = process::poll(pid) else {
            self.job = None;
            return true;
        };
        let mut changed = false;
        if !output.is_empty() {
            self.partial.push_str(&String::from_utf8_lossy(&output));
            while let Some(i) = self.partial.find('\n') {
                let line: String = self.partial.drain(..=i).collect();
                self.say(line.trim_end_matches('\n'), Kind::Output);
            }
            changed = true;
        }
        if let Some(status) = status {
            let rest = core::mem::take(&mut self.partial);
            if !rest.is_empty() {
                self.say(&rest, Kind::Output);
            }
            match status {
                Status::Exited(0) | Status::Killed => {}
                Status::Exited(code) => self.say(&format!("  {name}: exited with {code}"), Kind::Error),
                Status::Crashed(what) => self.say(&format!("  {name}: crashed: {what}"), Kind::Error),
            }
            self.job = None;
            changed = true;
        }
        changed
    }

    fn fail(&mut self, cmd: &str, what: &str, e: fs::Error) {
        self.say(&format!("  {cmd}: {what}: {}", e.text()), Kind::Error);
    }

    fn exec(&mut self, line: &str, out: &mut Vec<Request>) {
        // "echo text > file" and "echo text >> file"
        if let Some(rest) = line.strip_prefix("echo ") {
            if let Some((text, target)) = rest.split_once('>') {
                let append = target.starts_with('>');
                let target = target.trim_start_matches('>').trim();
                let path = self.path(target);
                let mut data = if append { fs::read(&path).unwrap_or_default() } else { Vec::new() };
                data.extend_from_slice(text.trim().as_bytes());
                data.push(b'\n');
                if let Err(e) = fs::write(&path, &data) {
                    self.fail("echo", target, e);
                }
                return;
            }
        }
        let (cmd, rest) = line.split_once(' ').map(|(c, r)| (c, r.trim())).unwrap_or((line, ""));
        match cmd {
            "help" => {
                for (name, what) in [
                    ("open <name>", "open an app, file or folder"),
                    ("ls [dir]", "list a directory"),
                    ("cd <dir>", "change directory"),
                    ("pwd", "print working directory"),
                    ("cat <file>", "print a file"),
                    ("mkdir <dir>", "make a directory"),
                    ("touch <file>", "create an empty file"),
                    ("rm <path>", "remove a file or empty directory"),
                    ("mv <from> <to>", "move or rename"),
                    ("df", "disk usage"),
                    ("<program>", "run a program from /Programs"),
                    ("ps", "running programs"),
                    ("kill <pid>", "stop a program"),
                    ("about", "version information"),
                    ("mem", "memory usage"),
                    ("cpu", "processor information"),
                    ("uptime", "time since boot"),
                    ("time", "date and time"),
                    ("echo <text>", "print text"),
                    ("clear", "clear the screen"),
                    ("close", "close this window"),
                    ("crash", "trigger a kernel fault"),
                    ("reboot", "restart"),
                    ("shutdown", "shut down"),
                ] {
                    self.say(&format!("  {name:<16}{what}"), Kind::Output);
                }
            }
            "about" | "version" => {
                self.say(&format!("Dante OS {VERSION}"), Kind::Accent);
                self.say("x86-64 kernel written in Rust", Kind::Output);
            }
            "apps" => {
                for k in AppKind::ALL {
                    self.say(&format!("  {:<13}{}", k.name(), k.describe()), Kind::Output);
                }
            }
            "open" => match AppKind::from_name(rest) {
                Some(k) => out.push(Request::Open(k)),
                None if rest.is_empty() => self.say("  usage: open <app|file|folder>", Kind::Error),
                None => {
                    let path = self.path(rest);
                    match fs::stat(&path) {
                        Ok(e) if e.dir => out.push(Request::OpenFolder(path)),
                        Ok(_) => out.push(Request::OpenFile(path)),
                        Err(e) => self.fail("open", rest, e),
                    }
                }
            },
            "ls" => {
                let path = self.path(rest);
                match fs::list(&path) {
                    Ok(entries) => {
                        for e in entries {
                            let size = if e.dir { String::new() } else { fs::size_text(e.size as u64) };
                            let when = time::DateTime::from_timestamp(e.modified).short_text();
                            let name = if e.dir { format!("{}/", e.name) } else { e.name };
                            self.say(&format!("  {name:<28}{size:>10}  {when}"), if name.ends_with('/') { Kind::Accent } else { Kind::Output });
                        }
                    }
                    Err(e) => self.fail("ls", rest, e),
                }
            }
            "cd" => {
                let path = self.path(if rest.is_empty() { "/" } else { rest });
                match fs::stat(&path) {
                    Ok(e) if e.dir => self.cwd = path,
                    Ok(_) => self.fail("cd", rest, fs::Error::NotDir),
                    Err(e) => self.fail("cd", rest, e),
                }
            }
            "pwd" => {
                let cwd = self.cwd.clone();
                self.say(&format!("  {cwd}"), Kind::Output);
            }
            "cat" => match fs::read(&self.path(rest)) {
                Ok(data) => {
                    let text = String::from_utf8_lossy(&data).to_string();
                    for l in text.trim_end_matches('\n').split('\n') {
                        self.say(l, Kind::Output);
                    }
                }
                Err(e) => self.fail("cat", rest, e),
            },
            "mkdir" => {
                if let Err(e) = fs::mkdir(&self.path(rest)) {
                    self.fail("mkdir", rest, e);
                }
            }
            "touch" => {
                let path = self.path(rest);
                if fs::stat(&path).is_err() {
                    if let Err(e) = fs::write(&path, &[]) {
                        self.fail("touch", rest, e);
                    }
                }
            }
            "rm" => {
                if let Err(e) = fs::remove(&self.path(rest)) {
                    self.fail("rm", rest, e);
                }
            }
            "mv" => match rest.split_once(' ') {
                Some((a, b)) => {
                    if let Err(e) = fs::rename(&self.path(a), &self.path(b.trim())) {
                        self.fail("mv", a, e);
                    }
                }
                None => self.say("  usage: mv <from> <to>", Kind::Error),
            },
            "df" => match fs::usage() {
                Some((used, total)) => {
                    self.say(&format!("  {} used of {}", fs::size_text(used), fs::size_text(total)), Kind::Output)
                }
                None => self.say("  df: no disk", Kind::Error),
            },
            "files" | "notes" | "clock" | "system" => out.push(Request::Open(AppKind::from_name(cmd).unwrap())),
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
            "ps" => {
                let running = process::list();
                if !running.is_empty() {
                    self.say(&format!("  {:<8}NAME", "PID"), Kind::Accent);
                }
                for (pid, name) in running {
                    self.say(&format!("  {pid:<8}{name}"), Kind::Output);
                }
            }
            "kill" => match rest.parse::<u32>() {
                Ok(pid) if process::kill(pid) => {}
                Ok(_) => self.say(&format!("  kill: {rest}: no such program"), Kind::Error),
                Err(_) => self.say("  usage: kill <pid>", Kind::Error),
            },
            _ => match self.program(cmd) {
                Some(path) => match process::spawn(&path, rest, &self.cwd) {
                    Ok(pid) => self.job = Some((pid, String::from(cmd.rsplit('/').next().unwrap_or(cmd)))),
                    Err(e) => self.say(&format!("  {cmd}: {e}"), Kind::Error),
                },
                None => self.say(&format!("  {cmd}: command not found"), Kind::Error),
            },
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
        if keyboard::ctrl() {
            if let Key::Char('c') = key {
                if let Some((pid, _)) = self.job {
                    process::kill(pid);
                }
                let line = format!("{}{}^C", self.lead(), self.input);
                self.input.clear();
                self.partial.clear();
                self.say(&line, Kind::Input);
                self.draw(c, area);
                return Some(area);
            }
            if let (Key::Char('d'), Some((pid, _))) = (key, &self.job) {
                // End of input: send what's typed, then nothing more.
                process::send_input(*pid, self.input.as_bytes());
                process::close_input(*pid);
                if !self.input.is_empty() {
                    let line = format!("{}{}", self.partial, self.input);
                    self.say(&line, Kind::Input);
                    self.input.clear();
                    self.partial.clear();
                }
                self.draw(c, area);
                return Some(area);
            }
            return None;
        }
        if let Some((pid, _)) = self.job {
            match key {
                Key::Char('\n') => {
                    let line = format!("{}{}", self.partial, self.input);
                    self.input.push('\n');
                    process::send_input(pid, self.input.as_bytes());
                    self.input.clear();
                    self.partial.clear();
                    self.say(&line, Kind::Input);
                    self.draw(c, area);
                    return Some(area);
                }
                Key::Char('\u{8}') => {
                    self.input.pop();
                }
                Key::Char(ch) if !ch.is_control() => {
                    if self.partial.chars().count() + self.input.chars().count() + 1 < self.cols {
                        self.input.push(ch);
                    }
                }
                _ => return None,
            }
            return Some(self.draw_input(c, area));
        }
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
                if self.prompt().chars().count() + self.input.chars().count() + 1 < self.cols {
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
        let prompt = self.prompt();
        self.say(&format!("{prompt}{cmd}"), Kind::Input);
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
        if self.pump() {
            self.draw(c, area);
            return Some(area);
        }
        if !self.focused || now - self.last_blink < BLINK_MS {
            return None;
        }
        self.last_blink = now;
        self.caret_on = !self.caret_on;
        Some(self.draw_input(c, area))
    }

    fn close(&mut self) {
        if let Some((pid, _)) = self.job.take() {
            process::detach(pid);
        }
    }

    fn focus(&mut self, focused: bool, c: &mut Canvas, area: Rect) -> Option<Rect> {
        self.focused = focused;
        self.caret_on = true;
        Some(self.draw_input(c, area))
    }
}
