//! The dock: a floating tray of apps along the bottom of the screen.
//! Open apps have a dot under them; the icon under the pointer lifts a little
//! and shows its name.

use alloc::vec::Vec;
use core::f32::consts::PI;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::apps::AppKind;
use crate::gfx::{Canvas, Rect, Rgb, Surface, text_width};
use crate::theme::*;

const ICON: i32 = 44;
const GAP: i32 = 12;
const PAD: i32 = 10;
const SEP: i32 = 21; // room for the divider before search
const SIDE: i32 = 24; // shadow room
const TOP: i32 = 46; // room above for the name tag and the lift
const BODY_H: i32 = ICON + 2 * PAD + 6;
const LIFT: f32 = 6.0;
const TILE: Rgb = Rgb::hex(0xF4F6F4);

#[derive(Clone, Copy, PartialEq)]
pub enum Item {
    App(AppKind),
    Search,
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::App(k) => k.title(),
            Item::Search => "Search",
        }
    }
}

pub struct Dock {
    pub canvas: Canvas,
    pub origin: (i32, i32),
    items: Vec<Item>,
    running: Vec<bool>,
    lift: Vec<f32>,
    hover: Option<usize>,
}

impl Dock {
    pub fn new(screen_w: usize, screen_h: usize) -> Self {
        let mut items: Vec<Item> = AppKind::ALL.iter().map(|&k| Item::App(k)).collect();
        items.push(Item::Search);
        let n = items.len() as i32;
        let body_w = 2 * PAD + n * ICON + (n - 2) * GAP + SEP;
        let (cw, ch) = (body_w + 2 * SIDE, TOP + BODY_H + SIDE);
        let origin = ((screen_w as i32 - cw) / 2, screen_h as i32 - 10 - BODY_H - TOP);
        let mut d = Self {
            canvas: Canvas::new(cw as usize, ch as usize, 0),
            origin,
            running: alloc::vec![false; items.len()],
            lift: alloc::vec![0.0; items.len()],
            items,
            hover: None,
        };
        d.paint();
        d
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.origin.0, self.origin.1, self.canvas.w as i32, self.canvas.h as i32)
    }

    fn body(&self) -> Rect {
        Rect::new(SIDE, TOP, self.canvas.w as i32 - 2 * SIDE, BODY_H)
    }

    /// The left edge of item `i`, in canvas coordinates.
    fn item_x(&self, i: usize) -> i32 {
        let last = self.items.len() - 1;
        SIDE + PAD + i as i32 * (ICON + GAP) - if i == last { GAP - SEP } else { 0 }
    }

    pub fn contains(&self, x: i32, y: i32) -> bool {
        self.body().offset(self.origin.0, self.origin.1).contains(x, y)
    }

    pub fn item_at(&self, x: i32, y: i32) -> Option<Item> {
        self.index_at(x, y).map(|i| self.items[i])
    }

    fn index_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.contains(x, y) {
            return None;
        }
        let lx = x - self.origin.0;
        (0..self.items.len()).find(|&i| lx >= self.item_x(i) - GAP / 2 && lx < self.item_x(i) + ICON + GAP / 2)
    }

    /// Returns true if the dock changed and needs redrawing.
    pub fn hover(&mut self, x: i32, y: i32) -> bool {
        let h = self.index_at(x, y);
        if h == self.hover {
            return false;
        }
        self.hover = h;
        self.paint();
        true
    }

    pub fn set_running(&mut self, running: impl Fn(AppKind) -> bool) -> bool {
        let next: Vec<bool> = self.items.iter().map(|i| matches!(i, Item::App(k) if running(*k))).collect();
        if next == self.running {
            return false;
        }
        self.running = next;
        self.paint();
        true
    }

    /// Eases the lift of hovered icons. True if it moved.
    pub fn tick(&mut self) -> bool {
        let mut moved = false;
        for i in 0..self.items.len() {
            let target = if self.hover == Some(i) { LIFT } else { 0.0 };
            let d = target - self.lift[i];
            if d.abs() > 0.05 {
                self.lift[i] += d * 0.3;
                if (target - self.lift[i]).abs() < 0.1 {
                    self.lift[i] = target;
                }
                moved = true;
            }
        }
        if moved {
            self.paint();
        }
        moved
    }

    fn paint(&mut self) {
        let body = self.body();
        let xs: Vec<i32> = (0..self.items.len()).map(|i| self.item_x(i)).collect();
        let c = &mut self.canvas;
        let all = c.bounds();
        c.fill(all, 0);
        c.panel(body, 18.0, PAPER, 0.78, Some(HAIRLINE), 0.12);

        let last = self.items.len() - 1;
        let sep_x = xs[last] - SEP / 2 - 1;
        c.fill_rgb(Rect::new(sep_x, body.y + PAD + 6, 1, ICON - 12), HAIRLINE);

        for (i, &item) in self.items.iter().enumerate() {
            let y = body.y + PAD - self.lift[i] as i32;
            draw_icon(c, item, xs[i], y);
            if self.running[i] {
                c.disc((xs[i] + ICON / 2) as f32, (body.bottom() - 6) as f32, 2.0, INK);
            }
        }

        if let Some(i) = self.hover {
            let label = self.items[i].label();
            let f = RasterHeight::Size16;
            let tw = text_width(label, f, FontWeight::Regular, 0) as i32;
            let tag = Rect::new(xs[i] + ICON / 2 - (tw + 20) / 2, body.y - 36, tw + 20, 26);
            c.panel(tag, 7.0, PAPER, 0.97, Some(HAIRLINE), 0.08);
            c.text((tag.x + 10) as usize, (tag.y + 5) as usize, label, f, FontWeight::Regular, INK, 0, 1.0);
        }
    }
}

fn draw_icon(c: &mut Canvas, item: Item, x: i32, y: i32) {
    let tile = Rect::new(x, y, ICON, ICON);
    let (cx, cy) = (x as f32 + ICON as f32 / 2.0, y as f32 + ICON as f32 / 2.0);
    match item {
        Item::App(AppKind::Terminal) => {
            c.panel(tile, 11.0, INK, 1.0, None, 0.0);
            let (x0, y0) = (x as f32 + 13.0, y as f32 + 15.0);
            c.stroke(x0, y0, x0 + 7.0, y0 + 6.0, PAPER);
            c.stroke(x0 + 7.0, y0 + 6.0, x0, y0 + 12.0, PAPER);
            c.fill_rgb(Rect::new(x + 23, y + 27, 9, 2), Rgb::hex(0x8FB3BD));
        }
        Item::App(AppKind::Notes) => {
            c.panel(tile, 11.0, TILE, 1.0, Some(HAIRLINE), 0.0);
            for k in 0..4 {
                let ly = y + 13 + k * 6;
                let len = if k == 3 { 12 } else { 22 };
                c.fill_rgb(Rect::new(x + 12, ly, len, 1), if k == 0 { INK } else { RULE });
            }
            c.fill_rgb(Rect::new(x + 9, y + 9, 1, 26), TERRACOTTA);
        }
        Item::App(AppKind::Clock) => {
            c.panel(tile, 11.0, TILE, 1.0, Some(HAIRLINE), 0.0);
            c.ring(cx, cy, 14.0, 1.2, RULE);
            for k in 0..12 {
                let a = k as f32 * PI / 6.0;
                let (s, co) = (libm::sinf(a), libm::cosf(a));
                let inner = if k % 3 == 0 { 10.0 } else { 11.5 };
                c.line(cx + s * inner, cy - co * inner, cx + s * 13.0, cy - co * 13.0, RULE, 1.0);
            }
            c.stroke(cx, cy, cx - 5.5, cy - 4.0, INK);
            c.stroke(cx, cy, cx + 6.0, cy - 7.5, INK);
            c.disc(cx, cy, 2.0, SEA);
        }
        Item::App(AppKind::System) => {
            c.panel(tile, 11.0, SEA, 1.0, None, 0.0);
            for k in 0..8 {
                let a = k as f32 * PI / 4.0;
                let (s, co) = (libm::sinf(a), libm::cosf(a));
                let (r0, r1, hw) = (8.0, 13.5, 2.6);
                let pts = [
                    (cx + s * r0 - co * hw, cy - co * r0 - s * hw),
                    (cx + s * r1 - co * hw * 0.8, cy - co * r1 - s * hw * 0.8),
                    (cx + s * r1 + co * hw * 0.8, cy - co * r1 + s * hw * 0.8),
                    (cx + s * r0 + co * hw, cy - co * r0 + s * hw),
                ];
                c.polygon(&pts, PAPER, 1.0);
            }
            c.disc(cx, cy, 10.0, PAPER);
            c.disc(cx, cy, 4.0, SEA);
        }
        Item::Search => {
            c.panel(tile, 11.0, TILE, 1.0, Some(HAIRLINE), 0.0);
            c.ring(cx - 2.5, cy - 2.5, 7.5, 1.8, INK);
            c.stroke(cx + 3.0, cy + 3.0, cx + 9.0, cy + 9.0, INK);
            c.stroke(cx + 3.5, cy + 2.5, cx + 9.5, cy + 8.5, INK);
        }
    }
}
