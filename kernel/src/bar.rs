//! The top bar: the Dante mark (which opens the system menu), the focused
//! app's name, search, and the date and time.

use alloc::format;
use alloc::string::String;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::gfx::{Canvas, Rect, Rgb, Surface, text_width};
use crate::theme::*;
use crate::time;

pub const BAR_H: i32 = 30;
const FONT: RasterHeight = RasterHeight::Size16;
const MARK_X: f32 = 24.0;
const ITEM_H: i32 = 30;
const MENU_W: i32 = 210;
const SHADOW: i32 = 24;
const SELECTED: Rgb = Rgb::hex(0xE4EBEC);

#[derive(Clone, Copy, PartialEq)]
pub enum Hit {
    Mark,
    Search,
}

pub struct TopBar {
    pub canvas: Canvas,
    w: i32,
    app: String,
    clock: String,
    mark_active: bool,
}

impl TopBar {
    pub fn new(w: usize) -> Self {
        let mut b = Self { canvas: Canvas::new(w, BAR_H as usize, 0), w: w as i32, app: String::new(), clock: clock_text(), mark_active: false };
        b.paint();
        b
    }

    pub fn rect(&self) -> Rect {
        Rect::new(0, 0, self.w, BAR_H)
    }

    fn search_x(&self) -> f32 {
        let cw = text_width(&self.clock, FONT, FontWeight::Regular, 0) as f32;
        self.w as f32 - 18.0 - cw - 26.0
    }

    pub fn hit(&self, x: i32, y: i32) -> Option<Hit> {
        if y >= BAR_H {
            return None;
        }
        if (x as f32 - MARK_X).abs() < 16.0 {
            return Some(Hit::Mark);
        }
        if (x as f32 - self.search_x()).abs() < 14.0 {
            return Some(Hit::Search);
        }
        None
    }

    /// Returns true if the bar needs to be redrawn on screen.
    pub fn set_app(&mut self, app: &str) -> bool {
        if self.app == app {
            return false;
        }
        self.app = String::from(app);
        self.paint();
        true
    }

    pub fn set_mark_active(&mut self, active: bool) -> bool {
        if self.mark_active == active {
            return false;
        }
        self.mark_active = active;
        self.paint();
        true
    }

    pub fn update_clock(&mut self) -> bool {
        let c = clock_text();
        if c == self.clock {
            return false;
        }
        self.clock = c;
        self.paint();
        true
    }

    fn paint(&mut self) {
        let w = self.w;
        let c = &mut self.canvas;
        let all = c.bounds();
        c.fill(all, PAPER.argb(214));
        c.fill(Rect::new(0, BAR_H - 1, w, 1), HAIRLINE.argb(230));

        // The mark: a sun on the horizon.
        if self.mark_active {
            c.panel(Rect::new(MARK_X as i32 - 14, 4, 28, BAR_H - 8), 6.0, SELECTED, 1.0, None, 0.0);
        }
        c.disc(MARK_X, 18.0, 6.5, SEA);
        let below = Rect::new(MARK_X as i32 - 10, 18, 21, 8);
        let bg = if self.mark_active { SELECTED.argb(255) } else { PAPER.argb(214) };
        c.fill(below, bg);
        c.fill_rgb(Rect::new(MARK_X as i32 - 10, 18, 21, 1), INK);

        if !self.app.is_empty() {
            c.text(46, 7, &self.app, FONT, FontWeight::Bold, INK, 0, 1.0);
        }

        let cw = text_width(&self.clock, FONT, FontWeight::Regular, 0) as i32;
        c.text((w - 18 - cw) as usize, 7, &self.clock, FONT, FontWeight::Regular, INK, 0, 1.0);

        // Search: a small magnifier.
        let sx = w as f32 - 18.0 - cw as f32 - 26.0;
        c.ring(sx - 1.5, 13.5, 4.8, 1.4, INK);
        c.line(sx + 2.0, 17.0, sx + 6.0, 21.0, INK, 1.0);
        c.line(sx + 2.4, 16.6, sx + 6.4, 20.6, INK, 1.0);
    }
}

fn clock_text() -> String {
    let n = time::now();
    let day = &time::DAYS[n.weekday()][..3];
    let month = time::MONTHS.get(n.month.saturating_sub(1) as usize).unwrap_or(&"");
    format!("{day} {} {month}  {:02}:{:02}", n.day, n.hour, n.minute)
}

// ---- The system menu, under the mark ----

#[derive(Clone, Copy)]
pub enum MenuItem {
    About,
    Search,
    Restart,
    ShutDown,
}

const ITEMS: [(MenuItem, &str); 4] =
    [(MenuItem::About, "About Dante OS"), (MenuItem::Search, "Search"), (MenuItem::Restart, "Restart"), (MenuItem::ShutDown, "Shut Down")];

pub struct Menu {
    pub canvas: Canvas,
    pub origin: (i32, i32),
    hover: Option<usize>,
}

impl Menu {
    pub fn new() -> Self {
        let h = ITEMS.len() as i32 * ITEM_H + 12 + 9;
        let mut m = Self {
            canvas: Canvas::new((MENU_W + 2 * SHADOW) as usize, (h + 2 * SHADOW) as usize, 0),
            origin: (8 - SHADOW, BAR_H + 4 - SHADOW),
            hover: None,
        };
        m.paint();
        m
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.origin.0, self.origin.1, self.canvas.w as i32, self.canvas.h as i32)
    }

    fn frame(&self) -> Rect {
        Rect::new(SHADOW, SHADOW, MENU_W, self.canvas.h as i32 - 2 * SHADOW)
    }

    fn item_y(i: usize) -> i32 {
        // A divider sits between "Search" and "Restart".
        SHADOW + 6 + i as i32 * ITEM_H + if i >= 2 { 9 } else { 0 }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.frame().offset(self.origin.0, self.origin.1).contains(x, y)
    }

    pub fn item_at(&self, x: i32, y: i32) -> Option<MenuItem> {
        self.index_at(x, y).map(|i| ITEMS[i].0)
    }

    fn index_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.contains(x, y) {
            return None;
        }
        let ly = y - self.origin.1;
        (0..ITEMS.len()).find(|&i| ly >= Self::item_y(i) && ly < Self::item_y(i) + ITEM_H)
    }

    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let h = self.index_at(x, y);
        if h == self.hover {
            return false;
        }
        self.hover = h;
        self.paint();
        true
    }

    fn paint(&mut self) {
        let frame = self.frame();
        let c = &mut self.canvas;
        let all = c.bounds();
        c.fill(all, 0);
        c.panel(frame, 9.0, PAPER, 0.97, Some(HAIRLINE), 0.10);
        for (i, (_, label)) in ITEMS.iter().enumerate() {
            let y = Self::item_y(i);
            if self.hover == Some(i) {
                c.panel(Rect::new(frame.x + 5, y, frame.w - 10, ITEM_H), 6.0, SELECTED, 1.0, None, 0.0);
            }
            c.text((frame.x + 16) as usize, (y + 7) as usize, label, FONT, FontWeight::Regular, INK, 0, 1.0);
        }
        c.fill_rgb(Rect::new(frame.x + 12, Self::item_y(2) - 5, frame.w - 24, 1), HAIRLINE);
    }
}
