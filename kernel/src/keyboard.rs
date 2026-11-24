//! PS/2 keyboard. The interrupt handler only queues raw scancodes; decoding
//! into characters happens outside the interrupt, in `next_key`.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use pc_keyboard::{DecodedKey, HandleControl, PS2Keyboard, ScancodeSet1, layouts::Us104Key};
use spin::Mutex;

pub use pc_keyboard::KeyCode;

const QUEUE: usize = 128;
static mut SCANCODES: [u8; QUEUE] = [0; QUEUE];
// Single producer (the interrupt) and single consumer (the kernel main loop).
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

pub fn push_scancode(code: u8) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % QUEUE;
    if next == TAIL.load(Ordering::Acquire) {
        return; // full: drop the key rather than block in an interrupt
    }
    unsafe { (&raw mut SCANCODES).cast::<u8>().add(head).write_volatile(code) };
    HEAD.store(next, Ordering::Release);
}

fn pop_scancode() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    if tail == HEAD.load(Ordering::Acquire) {
        return None;
    }
    let code = unsafe { (&raw const SCANCODES).cast::<u8>().add(tail).read_volatile() };
    TAIL.store((tail + 1) % QUEUE, Ordering::Release);
    Some(code)
}

static DECODER: Mutex<PS2Keyboard<Us104Key, ScancodeSet1>> =
    Mutex::new(PS2Keyboard::new(ScancodeSet1::new(), Us104Key, HandleControl::Ignore));

#[derive(Clone, Copy)]
pub enum Key {
    Char(char),
    Raw(KeyCode),
}

static CTRL: AtomicBool = AtomicBool::new(false);

/// Whether Ctrl was held for the key most recently returned by `next_key`.
pub fn ctrl() -> bool {
    CTRL.load(Ordering::Relaxed)
}

/// The next decoded key press, if any.
pub fn next_key() -> Option<Key> {
    let mut kb = DECODER.lock();
    while let Some(code) = pop_scancode() {
        if let Ok(Some(event)) = kb.add_byte(code) {
            let decoded = kb.process_keyevent(event);
            let m = kb.get_modifiers();
            CTRL.store(m.lctrl || m.rctrl, Ordering::Relaxed);
            match decoded {
                Some(DecodedKey::Unicode(c)) => return Some(Key::Char(c)),
                Some(DecodedKey::RawKey(k)) => return Some(Key::Raw(k)),
                None => {}
            }
        }
    }
    None
}
