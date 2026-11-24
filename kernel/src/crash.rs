//! The crash screen. When something goes wrong, Dante stops calmly and says
//! what happened, on screen and in the serial log.

use core::fmt::Write;
use noto_sans_mono_bitmap::{FontWeight, RasterHeight};

use crate::screen::Buf;
use crate::theme::*;

pub fn stop(what: &str, details: &[(&str, u64)]) -> ! {
    x86_64::instructions::interrupts::disable();

    crate::log!("");
    crate::log!("DANTE STOPPED: {what}");
    for (k, v) in details {
        crate::log!("  {k}: {v:#x}");
    }

    // The screen lock may be held by the code that crashed; take it anyway.
    let mut screen = unsafe {
        crate::SCREEN.force_unlock();
        crate::SCREEN.lock()
    };
    if let Some(s) = screen.as_mut() {
        draw(s, what, details);
    }
    crate::halt();
}

fn draw(s: &mut crate::screen::Screen, what: &str, details: &[(&str, u64)]) {
    let (w, h) = (s.width, s.height);
    let (x, y0) = (64usize, h / 3);
    s.clear_rect(0, 0, w, h);

    s.disc(x as f32 + 4.0, y0 as f32 - 30.0, 4.5, TERRACOTTA);
    s.text(x + 18, y0 - 40, "dante stopped", RasterHeight::Size16, FontWeight::Regular, MUTED, 0, 1.0);
    s.text(x, y0, what, RasterHeight::Size32, FontWeight::Regular, INK, 0, 1.0);

    let mut y = y0 + 64;
    s.rect(x, y, w - 2 * x, 1, HAIRLINE);
    y += 20;
    for (k, v) in details {
        let mut line: Buf<64> = Buf::new();
        let _ = write!(line, "{k:<10} {v:#018x}");
        s.text(x, y, line.as_str(), RasterHeight::Size16, FontWeight::Regular, INK, 0, 1.0);
        y += 26;
    }
    s.text(x, h - 64, "The machine is halted. Nothing was lost that wasn't already in memory. Restart to continue.", RasterHeight::Size16, FontWeight::Regular, MUTED, 0, 1.0);
}
