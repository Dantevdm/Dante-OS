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

pub const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
pub const DAYS: [&str; 7] = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

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

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

impl DateTime {
    /// Seconds since 1970 (in the machine's local time).
    pub fn timestamp(&self) -> u64 {
        let days = days_from_civil(self.year as i64, self.month as i64, self.day as i64);
        (days * 86_400 + self.hour as i64 * 3600 + self.minute as i64 * 60 + self.second as i64).max(0) as u64
    }

    pub fn from_timestamp(t: u64) -> DateTime {
        let days = (t / 86_400) as i64;
        let rem = t % 86_400;
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u8;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
        let year = (yoe + era * 400 + if month <= 2 { 1 } else { 0 }) as u16;
        DateTime { year, month, day, hour: (rem / 3600) as u8, minute: (rem / 60 % 60) as u8, second: (rem % 60) as u8 }
    }

    /// "24 Nov 08:46"
    pub fn short_text(&self) -> alloc::string::String {
        let month = MONTHS.get(self.month.saturating_sub(1) as usize).unwrap_or(&"?");
        alloc::format!("{} {month} {:02}:{:02}", self.day, self.hour, self.minute)
    }
}
