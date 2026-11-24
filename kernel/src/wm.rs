//! The window manager and compositor.
//!
//! Every layer lives in its own off-screen canvas: the wallpaper, each window,
//! the launcher and the pointer. When something changes we only recompose the
//! rectangles that changed, row by row, straight into the framebuffer. That
//! keeps it smooth without needing a GPU.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use crate::apps::{self, App, AppKind, Request};
use crate::boot_screen::ease_out;
use crate::desktop::{Desktop, MARGIN};
use crate::gfx::{Canvas, Rect, Surface, mix_rgb, over, rounded_rect_sdf};
use crate::keyboard::{self, Key, KeyCode};
use crate::launcher::{Action, Launcher, Outcome};
use crate::mouse::{self, Event};
use crate::theme::*;
use crate::{SCREEN, power, time};

/// Room around each window's paper for its shadow and corner marks.
const M: i32 = 24;
const TITLE_H: i32 = 48;
const PAD_X: i32 = 28;
const RADIUS: f32 = 10.0;
const OPEN_MS: u64 = 420;
const LIFT: i32 = 14;
const CURSOR: i32 = 10;

struct Window {
    id: u32,
    frame: Rect,
    surface: Canvas,
    app: Box<dyn App>,
    focused: bool,
    close_hover: bool,
    opened: u64,
}

impl Window {
    fn new(id: u32, x: i32, y: i32, app: Box<dyn App>, now: u64) -> Self {
        let (w, h) = app.size();
        let mut win = Window {
            id,
            frame: Rect::new(x, y, w, h),
            surface: Canvas::new((w + 2 * M) as usize, (h + 2 * M) as usize, 0),
            app,
            focused: false,
            close_hover: false,
            opened: now,
        };
        let all = win.surface.bounds();
        win.paint_base(all);
        win.paint_title();
        let area = win.content();
        win.app.draw(&mut win.surface, area);
        win
    }

    /// Where the app draws, in surface coordinates.
    fn content(&self) -> Rect {
        Rect::new(M + PAD_X, M + TITLE_H + 18, self.frame.w - 2 * PAD_X, self.frame.h - TITLE_H - 18 - 22)
    }

    fn progress(&self, now: u64) -> f32 {
        ease_out(((now - self.opened) as f32 / OPEN_MS as f32).min(1.0))
    }

    fn animating(&self, now: u64) -> bool {
        now - self.opened <= OPEN_MS + 32
    }

    fn alpha(&self, now: u64) -> u32 {
        (self.progress(now) * 255.0) as u32
    }

    /// Where the surface sits on screen right now (it rises as it opens).
    fn screen_rect(&self, now: u64) -> Rect {
        let lift = ((1.0 - self.progress(now)) * LIFT as f32) as i32;
        self.frame.expand(M).offset(0, lift)
    }

    /// Everything the window can cover, including its rise.
    fn full_rect(&self) -> Rect {
        let r = self.frame.expand(M);
        Rect::new(r.x, r.y, r.w, r.h + LIFT)
    }

    fn to_screen(&self, r: Rect, now: u64) -> Rect {
        let s = self.screen_rect(now);
        r.offset(s.x, s.y)
    }

    fn close_center(&self) -> (i32, i32) {
        (M + self.frame.w - 30, M + 25)
    }

    fn hits_close(&self, x: i32, y: i32) -> bool {
        let (cx, cy) = self.close_center();
        let (dx, dy) = (x - (self.frame.x - M + cx), y - (self.frame.y - M + cy));
        dx * dx + dy * dy <= 12 * 12
    }

    /// Shadow, paper and hairline border, computed per pixel for `region`.
    fn paint_base(&mut self, region: Rect) {
        let region = region.intersect(self.surface.bounds());
        let (fw, fh) = (self.frame.w as f32, self.frame.h as f32);
        let paper = PAPER.argb(255);
        let sw = self.surface.w;
        for py in region.y..region.bottom() {
            for px in region.x..region.right() {
                let (fx, fy) = ((px - M) as f32 + 0.5, (py - M) as f32 + 0.5);
                let d = rounded_rect_sdf(fx, fy, fw, fh, RADIUS);
                let i = py as usize * sw + px as usize;
                if d < -1.5 {
                    self.surface.px[i] = paper;
                    continue;
                }
                // A soft shadow, a little below the paper.
                let ds = rounded_rect_sdf(fx, fy - 6.0, fw, fh, RADIUS);
                let t = (1.0 - (ds + 4.0) / 22.0).clamp(0.0, 1.0);
                let sa = (0.12 * t * t * 255.0) as u32;
                let mut v = if sa > 0 { INK.argb(sa as u8) } else { 0 };
                let cover = (0.5 - d).clamp(0.0, 1.0);
                if cover > 0.0 {
                    let edge = (1.0 - (d + 0.5).abs()).clamp(0.0, 1.0);
                    v = over(v, PAPER.mix(HAIRLINE, edge).argb(0), (cover * 255.0 + 0.5) as u32);
                }
                self.surface.px[i] = v;
            }
        }
    }

    fn corner_regions(&self) -> [Rect; 2] {
        [Rect::new(M - 7, M - 7, 11, 11), Rect::new(M + self.frame.w - 4, M + self.frame.h - 4, 11, 11)]
    }

    /// Technical-drawing corner marks show which window has focus.
    fn paint_corners(&mut self) {
        for r in self.corner_regions() {
            self.paint_base(r);
        }
        if self.focused {
            let (w, h) = (self.frame.w, self.frame.h);
            let s = &mut self.surface;
            s.fill_rgb(Rect::new(M - 6, M - 6, 9, 1), RULE);
            s.fill_rgb(Rect::new(M - 6, M - 6, 1, 9), RULE);
            s.fill_rgb(Rect::new(M + w - 3, M + h + 5, 9, 1), RULE);
            s.fill_rgb(Rect::new(M + w + 5, M + h - 3, 1, 9), RULE);
        }
    }

    fn title_rect(&self) -> Rect {
        Rect::new(M + 2, M + 2, self.frame.w - 4, TITLE_H - 1)
    }

    fn paint_title(&mut self) {
        let r = self.title_rect();
        self.paint_base(r);
        let f = noto_sans_mono_bitmap::RasterHeight::Size16;
        let wt = noto_sans_mono_bitmap::FontWeight::Regular;
        let (num, name) = if self.focused { (SEA, INK) } else { (MUTED, MUTED) };
        let (x, y) = ((M + PAD_X) as usize, (M + 17) as usize);
        let (cx, cy) = self.close_center();
        let s = &mut self.surface;
        let x = s.text(x, y, &format!("{:02}", self.id), f, wt, num, 0, 1.0);
        let x = s.text(x + 12, y, self.app.kind().name(), f, wt, name, 0, 1.0);
        let sub = self.app.subtitle();
        if !sub.is_empty() {
            s.text(x + 12, y, &format!("/ {sub}"), f, noto_sans_mono_bitmap::FontWeight::Light, MUTED, 0, 1.0);
        }
        if self.close_hover {
            s.disc(cx as f32, cy as f32, 6.5, TERRACOTTA);
            s.line(cx as f32 - 2.5, cy as f32 - 2.5, cx as f32 + 2.5, cy as f32 + 2.5, PAPER, 1.0);
            s.line(cx as f32 - 2.5, cy as f32 + 2.5, cx as f32 + 2.5, cy as f32 - 2.5, PAPER, 1.0);
        } else {
            s.ring(cx as f32, cy as f32, 5.5, 1.2, if self.focused { RULE } else { HAIRLINE });
        }
        s.fill_rgb(Rect::new(M + 1, M + TITLE_H, self.frame.w - 2, 1), HAIRLINE);
    }

    fn set_focus(&mut self, focused: bool) -> bool {
        if self.focused == focused {
            return false;
        }
        self.focused = focused;
        self.paint_corners();
        self.paint_title();
        let area = self.content();
        self.app.focus(focused, &mut self.surface, area);
        true
    }
}

/// Blend one row of a layer into `row` (which starts at screen x `rx`).
fn layer(row: &mut [u32], rx: i32, y: i32, c: &Canvas, cx: i32, cy: i32, alpha: u32) {
    if alpha == 0 || y < cy || y >= cy + c.h as i32 {
        return;
    }
    let x0 = rx.max(cx);
    let x1 = (rx + row.len() as i32).min(cx + c.w as i32);
    if x1 <= x0 {
        return;
    }
    let start = (y - cy) as usize * c.w + (x0 - cx) as usize;
    let src = &c.px[start..start + (x1 - x0) as usize];
    let dst = &mut row[(x0 - rx) as usize..(x1 - rx) as usize];
    for (d, &s) in dst.iter_mut().zip(src) {
        let a = if alpha >= 255 { s >> 24 } else { (s >> 24) * alpha / 255 };
        if a >= 255 {
            *d = s;
        } else if a > 0 {
            *d = 0xFF00_0000 | mix_rgb(*d, s, a);
        }
    }
}

fn clock_text() -> String {
    let n = time::now();
    format!("{:02}:{:02}", n.hour, n.minute)
}

pub struct Wm {
    w: i32,
    h: i32,
    wallpaper: Canvas,
    desktop: Desktop,
    windows: Vec<Window>,
    focused: bool,
    launcher: Option<Launcher>,
    pointer: (i32, i32),
    pointer_img: Canvas,
    drag: Option<(i32, i32)>,
    buttons: u8,
    dirty: Vec<Rect>,
    row: Vec<u32>,
    next_id: u32,
    last_tick: u64,
    last_ambient: u64,
    ambient_frame: u64,
    last_plane: Rect,
    last_clock_check: u64,
}

impl Wm {
    pub fn new(now: u64) -> Self {
        let (w, h) = {
            let g = SCREEN.lock();
            let s = g.as_ref().expect("no screen");
            (s.width, s.height)
        };
        let mut desktop = Desktop::new(w, h);
        desktop.clock = clock_text();
        let mut wallpaper = Canvas::new(w, h, 0xFF00_0000);
        let all = wallpaper.bounds();
        desktop.paint(&mut wallpaper, all, now);

        // The pointer: a small ink dot with a paper ring, like a pencil mark.
        let size = (CURSOR * 2) as usize;
        let mut pointer_img = Canvas::new(size, size, 0);
        pointer_img.disc_alpha(CURSOR as f32, CURSOR as f32, 7.0, PAPER, 0.9);
        pointer_img.disc(CURSOR as f32, CURSOR as f32, 4.5, INK);

        Self {
            w: w as i32,
            h: h as i32,
            last_plane: desktop.plane_rect(now),
            wallpaper,
            desktop,
            windows: Vec::new(),
            focused: false,
            launcher: None,
            pointer: (w as i32 / 2, h as i32 / 2),
            pointer_img,
            drag: None,
            buttons: 0,
            dirty: vec![all],
            row: vec![0; w],
            next_id: 1,
            last_tick: 0,
            last_ambient: 0,
            ambient_frame: 0,
            last_clock_check: now,
        }
    }

    fn screen(&self) -> Rect {
        Rect::new(0, 0, self.w, self.h)
    }

    fn pointer_rect(&self) -> Rect {
        Rect::new(self.pointer.0 - CURSOR, self.pointer.1 - CURSOR, 2 * CURSOR, 2 * CURSOR)
    }

    // ---- Windows ----

    pub fn open(&mut self, kind: AppKind, now: u64) {
        let app = apps::create(kind);
        let (fw, fh) = app.size();
        let k = (self.next_id as i32 - 1) % 6;
        let x = (MARGIN + k * 48).min(self.w - fw - 24).max(8);
        let y = (76 + k * 34).min(self.h - fh - 24).max(56);
        let win = Window::new(self.next_id, x, y, app, now);
        self.next_id += 1;
        self.dirty.push(win.full_rect());
        self.windows.push(win);
        self.focus_top(true);
        self.update_footer(now);
        crate::log!("wm: opened {} ({} windows)", kind.name(), self.windows.len());
    }

    fn close(&mut self, index: usize, now: u64) {
        let win = self.windows.remove(index);
        self.dirty.push(win.full_rect());
        if self.drag.is_some() {
            self.drag = None;
        }
        let keep = self.focused;
        self.focused = false;
        self.focus_top(keep);
        self.update_footer(now);
        crate::log!("wm: closed {} ({} windows)", win.app.kind().name(), self.windows.len());
    }

    fn raise(&mut self, index: usize, now: u64) {
        if index + 1 != self.windows.len() {
            let win = self.windows.remove(index);
            self.dirty.push(win.screen_rect(now));
            self.windows.push(win);
        }
    }

    /// Give focus to the top window (or to nothing).
    fn focus_top(&mut self, focused: bool) {
        self.focused = focused && !self.windows.is_empty();
        let top = self.windows.len().saturating_sub(1);
        let now = time::millis();
        for (i, win) in self.windows.iter_mut().enumerate() {
            if win.set_focus(self.focused && i == top) {
                self.dirty.push(win.screen_rect(now));
            }
        }
    }

    fn window_at(&self, x: i32, y: i32) -> Option<usize> {
        self.windows.iter().rposition(|w| w.frame.contains(x, y))
    }

    fn update_footer(&mut self, now: u64) {
        self.desktop.open = self.windows.len();
        let r = self.desktop.footer_rect();
        self.desktop.paint(&mut self.wallpaper, r, now);
        self.dirty.push(r);
    }

    /// Carry out what an app asked for. `id` is the window that asked.
    fn handle(&mut self, id: u32, requests: Vec<Request>, now: u64) {
        for r in requests {
            match r {
                Request::Open(kind) => self.open(kind, now),
                Request::Close => {
                    if let Some(i) = self.windows.iter().position(|w| w.id == id) {
                        self.close(i, now);
                    }
                }
                Request::Reboot => power::reboot(),
                Request::Shutdown => power::shutdown(),
            }
        }
    }

    fn act(&mut self, action: Action, now: u64) {
        match action {
            Action::Open(kind) => self.open(kind, now),
            Action::Run(cmd) => {
                let i = match self.windows.iter().rposition(|w| w.app.kind() == AppKind::Terminal) {
                    Some(i) => i,
                    None => {
                        self.open(AppKind::Terminal, now);
                        self.windows.len() - 1
                    }
                };
                self.raise(i, now);
                self.focus_top(true);
                let win = self.windows.last_mut().unwrap();
                let area = win.content();
                let mut out = Vec::new();
                if let Some(r) = win.app.run(&cmd, &mut win.surface, area, &mut out) {
                    self.dirty.push(win.to_screen(r, now));
                }
                let id = win.id;
                self.handle(id, out, now);
            }
            Action::Reboot => power::reboot(),
            Action::Shutdown => power::shutdown(),
        }
    }

    // ---- Launcher ----

    fn open_launcher(&mut self, query: &str, now: u64) {
        self.drag = None;
        self.launcher = Some(Launcher::new(self.w as usize, self.h as usize, query, now));
        self.dirty.push(self.screen());
    }

    fn close_launcher(&mut self) {
        self.launcher = None;
        self.dirty.push(self.screen());
    }

    // ---- Input ----

    fn on_key(&mut self, key: Key, now: u64) {
        if let Some(l) = self.launcher.as_mut() {
            match l.key(key) {
                Outcome::Nothing => {}
                Outcome::Redraw => self.dirty.push(self.screen()),
                Outcome::Close => self.close_launcher(),
                Outcome::Activate(a) => {
                    self.close_launcher();
                    self.act(a, now);
                }
            }
            return;
        }
        let ctrl = keyboard::ctrl();
        match key {
            Key::Raw(KeyCode::LWin | KeyCode::RWin) => return self.open_launcher("", now),
            Key::Char(' ') if ctrl => return self.open_launcher("", now),
            Key::Char('w') if ctrl => {
                if self.focused && !self.windows.is_empty() {
                    self.close(self.windows.len() - 1, now);
                }
                return;
            }
            _ => {}
        }
        if self.focused {
            if let Some(win) = self.windows.last_mut() {
                let area = win.content();
                let mut out = Vec::new();
                if let Some(r) = win.app.key(key, &mut win.surface, area, &mut out) {
                    self.dirty.push(win.to_screen(r, now));
                }
                let id = win.id;
                self.handle(id, out, now);
                return;
            }
        }
        // Typing on the desktop starts a search.
        if let Key::Char(c) = key {
            if !c.is_control() {
                self.open_launcher(&c.to_string(), now);
            }
        }
    }

    fn on_mouse(&mut self, e: Event, now: u64) {
        let (x, y, buttons) = match e {
            Event::Abs { x, y, buttons, .. } => {
                ((x as i64 * self.w as i64 / 0x1_0000) as i32, (y as i64 * self.h as i64 / 0x1_0000) as i32, buttons)
            }
            Event::Rel { dx, dy, buttons, .. } => (
                (self.pointer.0 + dx as i32).clamp(0, self.w - 1),
                (self.pointer.1 + dy as i32).clamp(0, self.h - 1),
                buttons,
            ),
        };
        if (x, y) != self.pointer {
            self.move_pointer(x, y, now);
        }
        let pressed = buttons & !self.buttons;
        let released = self.buttons & !buttons;
        self.buttons = buttons;
        if pressed & mouse::LEFT != 0 {
            self.press(x, y, now);
        }
        if released & mouse::LEFT != 0 {
            self.drag = None;
        }
    }

    fn move_pointer(&mut self, x: i32, y: i32, now: u64) {
        self.dirty.push(self.pointer_rect());
        self.pointer = (x, y);
        self.dirty.push(self.pointer_rect());

        if let Some((gx, gy)) = self.drag {
            if let Some(win) = self.windows.last_mut() {
                let before = win.screen_rect(now);
                win.frame.x = (x - gx).clamp(-win.frame.w + 120, self.w - 120);
                win.frame.y = (y - gy).clamp(0, self.h - 60);
                self.dirty.push(before);
                self.dirty.push(win.screen_rect(now));
            }
            return;
        }

        if let Some(l) = self.launcher.as_mut() {
            if l.hover(x, y) {
                self.dirty.push(self.screen());
            }
            return;
        }

        let hit = self.window_at(x, y);
        for (i, win) in self.windows.iter_mut().enumerate() {
            let hover = hit == Some(i) && win.hits_close(x, y);
            if hover != win.close_hover {
                win.close_hover = hover;
                win.paint_title();
                let r = win.title_rect();
                self.dirty.push(win.to_screen(r, now));
            }
        }
    }

    fn press(&mut self, x: i32, y: i32, now: u64) {
        if let Some(l) = self.launcher.as_ref() {
            match l.row_at(x, y).and_then(|i| l.action(i)) {
                Some(a) => {
                    self.close_launcher();
                    self.act(a, now);
                }
                None if y < 60 => self.close_launcher(),
                None => {}
            }
            return;
        }

        let Some(i) = self.window_at(x, y) else {
            if self.desktop.launcher_hit(x, y) {
                self.open_launcher("", now);
            } else {
                self.focus_top(false);
            }
            return;
        };
        self.raise(i, now);
        self.focus_top(true);
        let top = self.windows.len() - 1;
        if self.windows[top].hits_close(x, y) {
            self.close(top, now);
            return;
        }
        let win = &mut self.windows[top];
        let (lx, ly) = (x - win.frame.x, y - win.frame.y);
        if ly < TITLE_H {
            self.drag = Some((lx, ly));
            return;
        }
        let area = win.content();
        let mut out = Vec::new();
        if let Some(r) = win.app.click(lx + M, ly + M, &mut win.surface, area, &mut out) {
            self.dirty.push(win.to_screen(r, now));
        }
        let id = win.id;
        self.handle(id, out, now);
    }

    // ---- Time ----

    fn tick(&mut self, now: u64) {
        for win in self.windows.iter_mut() {
            if win.animating(now) {
                self.dirty.push(win.full_rect());
            }
            let area = win.content();
            if let Some(r) = win.app.tick(now, &mut win.surface, area) {
                self.dirty.push(win.to_screen(r, now));
            }
        }
        if self.launcher.as_ref().is_some_and(|l| l.animating(now)) {
            self.dirty.push(self.screen());
        }

        // The wallpaper's slow life: the plane often, the sun and waves less.
        if now - self.last_ambient >= 50 {
            self.last_ambient = now;
            self.ambient_frame += 1;
            let plane = self.desktop.plane_rect(now);
            let mut regions = vec![plane.union(self.last_plane)];
            self.last_plane = plane;
            if self.ambient_frame % 2 == 0 {
                regions.push(self.desktop.sun_rect());
                regions.extend(self.desktop.wave_rects());
            }
            for r in regions {
                self.desktop.paint(&mut self.wallpaper, r, now);
                self.dirty.push(r);
            }
        }

        if now - self.last_clock_check >= 1000 {
            self.last_clock_check = now;
            let clock = clock_text();
            if clock != self.desktop.clock {
                self.desktop.clock = clock;
                let r = self.desktop.header_rect();
                self.desktop.paint(&mut self.wallpaper, r, now);
                self.dirty.push(r);
            }
        }
    }

    // ---- Composition ----

    fn flush(&mut self, now: u64) {
        let screen = self.screen();
        let mut rects: Vec<Rect> = core::mem::take(&mut self.dirty)
            .into_iter()
            .map(|r| r.intersect(screen))
            .filter(|r| !r.is_empty())
            .collect();
        // Merge overlapping rectangles so no pixel is composed twice.
        let mut i = 0;
        while i < rects.len() {
            let mut merged = false;
            let mut j = i + 1;
            while j < rects.len() {
                if rects[i].expand(2).intersects(rects[j]) {
                    rects[i] = rects[i].union(rects[j]);
                    rects.swap_remove(j);
                    merged = true;
                } else {
                    j += 1;
                }
            }
            if !merged {
                i += 1;
            }
        }
        for r in rects {
            self.compose(r, now);
        }
    }

    fn compose(&mut self, r: Rect, now: u64) {
        let mut guard = SCREEN.lock();
        let Some(screen) = guard.as_mut() else { return };
        let launcher_alpha = self.launcher.as_ref().map(|l| l.alpha(now));
        let pointer = self.pointer_rect();
        let row = &mut self.row[..r.w as usize];
        for y in r.y..r.bottom() {
            let start = y as usize * self.wallpaper.w + r.x as usize;
            row.copy_from_slice(&self.wallpaper.px[start..start + r.w as usize]);
            for win in &self.windows {
                let s = win.screen_rect(now);
                layer(row, r.x, y, &win.surface, s.x, s.y, win.alpha(now));
            }
            if let (Some(l), Some(a)) = (self.launcher.as_ref(), launcher_alpha) {
                layer(row, r.x, y, &l.canvas, 0, 0, a);
            }
            layer(row, r.x, y, &self.pointer_img, pointer.x, pointer.y, 255);
            screen.write_row(r.x as usize, y as usize, row);
        }
    }

    pub fn run(&mut self) -> ! {
        loop {
            let now = time::millis();
            while let Some(k) = keyboard::next_key() {
                self.on_key(k, now);
            }
            while let Some(e) = mouse::next_event() {
                self.on_mouse(e, now);
            }
            if now - self.last_tick >= 16 {
                self.last_tick = now;
                self.tick(now);
            }
            if !self.dirty.is_empty() {
                self.flush(now);
            }
            x86_64::instructions::hlt();
        }
    }
}
