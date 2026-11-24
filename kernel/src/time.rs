//! Time: a 1000 Hz tick from the Local APIC timer, plus wall-clock time from
//! the battery-backed CMOS clock.

use core::sync::atomic::{AtomicU64, Ordering};
use x86_64::instructions::port::Port;

pub const HZ: u64 = 1000;
static TICKS: AtomicU64 = AtomicU64::new(0);

pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

/// Milliseconds since the timer started.
pub fn millis() -> u64 {
    TICKS.load(Ordering::Relaxed) * 1000 / HZ
}

/// Sleep, letting the CPU rest between ticks.
pub fn sleep_ms(ms: u64) {
    let until = millis() + ms;
    while millis() < until {
        x86_64::instructions::hlt();
    }
}

/// Wall-clock time from the battery-backed CMOS real-time clock.
#[derive(Clone, Copy)]
pub struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
}

fn cmos(reg: u8) -> u8 {
    unsafe {
        Port::<u8>::new(0x70).write(reg);
        Port::<u8>::new(0x71).read()
    }
}

pub fn now() -> DateTime {
    // Wait until the clock is not mid-update, then read twice until stable.
    let read = || {
        while cmos(0x0A) & 0x80 != 0 {}
        [cmos(0x00), cmos(0x02), cmos(0x04), cmos(0x07), cmos(0x08), cmos(0x09)]
    };
    let mut a = read();
    loop {
        let b = read();
        if a == b {
            break;
        }
        a = b;
    }
    let status_b = cmos(0x0B);
    let bcd = status_b & 0x04 == 0;
    let conv = |v: u8| if bcd { (v & 0x0F) + (v >> 4) * 10 } else { v };
    let mut hour = conv(a[2] & 0x7F);
    if status_b & 0x02 == 0 && a[2] & 0x80 != 0 {
        hour = (hour + 12) % 24; // 12-hour clock, PM
    }
    DateTime {
        second: conv(a[0]),
        minute: conv(a[1]),
        hour,
        day: conv(a[3]),
        month: conv(a[4]),
        year: 2000 + conv(a[5]) as u16,
    }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

impl DateTime {
    /// Day of the week, 0 = Sunday (Sakamoto's method).
    pub fn weekday(&self) -> usize {
        const T: [u16; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
        let m = self.month.clamp(1, 12) as usize;
        let y = if m < 3 { self.year - 1 } else { self.year } as usize;
        (y + y / 4 - y / 100 + y / 400 + T[m - 1] as usize + self.day as usize) % 7
    }

    /// "Monday 23 Nov 2026"
    pub fn date_text(&self) -> alloc::string::String {
        let month = MONTHS.get(self.month.saturating_sub(1) as usize).unwrap_or(&"?");
        alloc::format!("{} {} {month} {}", DAYS[self.weekday()], self.day, self.year)
    }
}
