//! Apps. Each one draws into its window's content area and reacts to keys,
//! clicks and the passing of time. Apps ask the window manager for things
//! (open another app, close, power off) through `Request`s.

mod clock;
mod files;
mod notes;
mod system;
mod terminal;

use alloc::boxed::Box;
use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicUsize, Ordering};

use crate::gfx::{Canvas, Rect};
use crate::keyboard::Key;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppKind {
    Files,
    Terminal,
    Notes,
    Clock,
    System,
}

impl AppKind {
    pub const ALL: [AppKind; 5] = [AppKind::Files, AppKind::Terminal, AppKind::Notes, AppKind::Clock, AppKind::System];

    pub fn name(self) -> &'static str {
        match self {
            AppKind::Files => "files",
            AppKind::Terminal => "terminal",
            AppKind::Notes => "notes",
            AppKind::Clock => "clock",
            AppKind::System => "system",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            AppKind::Files => "Files",
            AppKind::Terminal => "Terminal",
            AppKind::Notes => "Notes",
            AppKind::Clock => "Clock",
            AppKind::System => "System",
        }
    }

    pub fn describe(self) -> &'static str {
        match self {
            AppKind::Files => "File browser",
            AppKind::Terminal => "Command line",
            AppKind::Notes => "Text notes",
            AppKind::Clock => "Clock",
            AppKind::System => "System information",
        }
    }

    pub fn from_name(name: &str) -> Option<AppKind> {
        AppKind::ALL.into_iter().find(|k| k.name() == name)
    }
}

pub enum Request {
    Open(AppKind),
    OpenFile(String),
    OpenFolder(String),
    Close,
    Reboot,
    Shutdown,
}

pub trait App {
    fn kind(&self) -> AppKind;
    /// Preferred window size (the paper, including the title bar).
    fn size(&self) -> (i32, i32);
    /// The window's title. Apps showing a document use its name.
    fn title(&self) -> String {
        String::from(self.kind().title())
    }
    /// The file this window is showing, if any.
    fn file(&self) -> Option<&str> {
        None
    }
    /// Called before the window closes, e.g. to save.
    fn close(&mut self) {}
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
        AppKind::Files => Box::new(files::Files::new("/")),
        AppKind::Notes => Box::new(notes::Notes::latest()),
        AppKind::Clock => Box::new(clock::Clock::new()),
        AppKind::System => Box::new(system::System::new()),
    }
}

/// Open a document in the app that handles it (Notes, for now).
pub fn open_file(path: &str) -> Box<dyn App> {
    Box::new(notes::Notes::open(path))
}

pub fn open_folder(path: &str) -> Box<dyn App> {
    Box::new(files::Files::new(path))
}

pub fn new_note() -> Box<dyn App> {
    Box::new(notes::Notes::blank())
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
