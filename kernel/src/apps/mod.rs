//! Apps. Each one draws into its window's content area and reacts to keys,
//! clicks and the passing of time. Apps ask the window manager for things
//! (open another app, close, power off) through `Request`s.

mod clock;
mod notes;
mod system;
mod terminal;

use alloc::boxed::Box;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::gfx::{Canvas, Rect};
use crate::keyboard::Key;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppKind {
    Terminal,
    Notes,
    Clock,
    System,
}

impl AppKind {
    pub const ALL: [AppKind; 4] = [AppKind::Terminal, AppKind::Notes, AppKind::Clock, AppKind::System];

    pub fn name(self) -> &'static str {
        match self {
            AppKind::Terminal => "terminal",
            AppKind::Notes => "notes",
            AppKind::Clock => "clock",
            AppKind::System => "system",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            AppKind::Terminal => "commands for the kernel",
            AppKind::Notes => "write something down",
            AppKind::Clock => "the time, calmly",
            AppKind::System => "what this machine is",
        }
    }

    pub fn from_name(name: &str) -> Option<AppKind> {
        AppKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

pub enum Request {
    Open(AppKind),
    Close,
    Reboot,
    Shutdown,
}

pub trait App {
    fn kind(&self) -> AppKind;
    /// Preferred window size (the paper, including the title bar).
    fn size(&self) -> (i32, i32);
    /// A short note shown after the title, like a path.
    fn subtitle(&self) -> &str {
        ""
    }
    /// Draw the whole content area.
    fn draw(&mut self, c: &mut Canvas, area: Rect);

    // The handlers below draw what changed and return the part they touched.

    fn key(&mut self, _key: Key, _c: &mut Canvas, _area: Rect, _out: &mut Vec<Request>) -> Option<Rect> {
        None
    }
    fn click(&mut self, _x: i32, _y: i32, _c: &mut Canvas, _area: Rect, _out: &mut Vec<Request>) -> Option<Rect> {
        None
    }
    fn tick(&mut self, _now: u64, _c: &mut Canvas, _area: Rect) -> Option<Rect> {
        None
    }
    fn focus(&mut self, _focused: bool, _c: &mut Canvas, _area: Rect) -> Option<Rect> {
        None
    }
    /// Run a shell command (only the terminal does this).
    fn run(&mut self, _cmd: &str, _c: &mut Canvas, _area: Rect, _out: &mut Vec<Request>) -> Option<Rect> {
        None
    }
}

pub fn create(kind: AppKind) -> Box<dyn App> {
    match kind {
        AppKind::Terminal => Box::new(terminal::Terminal::new()),
        AppKind::Notes => Box::new(notes::Notes::new()),
        AppKind::Clock => Box::new(clock::Clock::new()),
        AppKind::System => Box::new(system::System::new()),
    }
}

// ---- Facts the apps show, set once at boot ----

pub static CORES: AtomicUsize = AtomicUsize::new(1);
pub static DISPLAY_W: AtomicUsize = AtomicUsize::new(0);
pub static DISPLAY_H: AtomicUsize = AtomicUsize::new(0);

pub fn cores() -> usize {
    CORES.load(Ordering::Relaxed)
}

pub fn uptime_text() -> alloc::string::String {
    let ms = crate::time::millis();
    let s = ms / 1000;
    alloc::format!("{}h {:02}m {:02}s", s / 3600, s / 60 % 60, s % 60)
}

/// The caret blink, shared by the text apps.
pub const BLINK_MS: u64 = 530;
