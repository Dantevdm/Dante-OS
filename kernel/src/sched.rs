//! The scheduler: takes turns between the desktop and running programs.
//!
//! Every timer tick (1 ms) saves the registers of whatever was running and
//! resumes the next thread that is ready. Thread 0 is the kernel itself (the
//! desktop); every other thread belongs to a program.
//!
//! Rules that keep this safe on one core:
//! - The interrupt side only ever `try_lock`s the table, and never allocates.
//! - Normal code never allocates while holding the table lock, so a thread
//!   paused inside the allocator can always be resumed to finish.
//! - A thread is only stopped at the user-mode boundary, never while it may
//!   hold a kernel lock.

use alloc::boxed::Box;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::asm;
use spin::Mutex;
use x86_64::registers::control::Cr3;

use crate::{apic, gdt, time};

pub const MAX_THREADS: usize = 32;
const KERNEL_STACK: usize = 32 * 1024;
pub const YIELD_VECTOR: u8 = 0x33;

/// The registers saved on a thread's stack when it is interrupted, in the
/// order the entry code pushes them, followed by what the CPU pushes.
#[repr(C)]
#[derive(Default)]
pub struct Regs {
    pub r15: u64,
    pub r14: u64,
    pub r13: u64,
    pub r12: u64,
    pub r11: u64,
    pub r10: u64,
    pub r9: u64,
    pub r8: u64,
    pub rbp: u64,
    pub rdi: u64,
    pub rsi: u64,
    pub rdx: u64,
    pub rcx: u64,
    pub rbx: u64,
    pub rax: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// An interrupt entry point that saves every register, calls `$handler` with
/// the stack pointer, and resumes whichever stack the handler returns.
macro_rules! entry {
    ($name:ident, $handler:path) => {
        #[unsafe(naked)]
        pub extern "C" fn $name() {
            core::arch::naked_asm!(
                "push rax", "push rbx", "push rcx", "push rdx", "push rsi", "push rdi", "push rbp",
                "push r8", "push r9", "push r10", "push r11", "push r12", "push r13", "push r14", "push r15",
                "cld",
                "mov rdi, rsp",
                "call {handler}",
                "mov rsp, rax",
                "pop r15", "pop r14", "pop r13", "pop r12", "pop r11", "pop r10", "pop r9", "pop r8",
                "pop rbp", "pop rdi", "pop rsi", "pop rdx", "pop rcx", "pop rbx", "pop rax",
                "iretq",
                handler = sym $handler,
            );
        }
    };
}
pub(crate) use entry;

entry!(timer_entry, on_timer);
entry!(yield_entry, on_yield);

#[derive(Clone, Copy, PartialEq)]
enum State {
    Ready,
    Sleeping(u64),
    Dead,
}

struct Thread {
    pid: u32,
    rsp: u64,
    cr3: u64,
    stack_top: u64,
    _stack: Option<Box<[u8]>>,
    state: State,
    kill: bool,
}

struct Table {
    slots: [Option<Thread>; MAX_THREADS],
    current: usize,
}

static TABLE: Mutex<Table> = Mutex::new(Table { slots: [const { None }; MAX_THREADS], current: 0 });

/// Register the code running now (the kernel's main loop) as thread 0.
pub fn init() {
    let cr3 = Cr3::read_raw().0.start_address().as_u64();
    TABLE.lock().slots[0] = Some(Thread { pid: 0, rsp: 0, cr3, stack_top: 0, _stack: None, state: State::Ready, kill: false });
}

extern "C" fn on_timer(rsp: u64) -> u64 {
    time::tick();
    apic::eoi();
    switch(rsp)
}

extern "C" fn on_yield(rsp: u64) -> u64 {
    switch(rsp)
}

fn switch(rsp: u64) -> u64 {
    let Some(mut t) = TABLE.try_lock() else { return rsp };
    let now = time::millis();
    let cur = t.current;
    let from_user = unsafe { (*(rsp as *const Regs)).cs } & 3 == 3;
    if let Some(th) = t.slots[cur].as_mut() {
        th.rsp = rsp;
        if th.kill && from_user {
            th.state = State::Dead;
        }
    }

    let mut next = None;
    for i in 1..=MAX_THREADS {
        let idx = (cur + i) % MAX_THREADS;
        if let Some(th) = t.slots[idx].as_mut() {
            if let State::Sleeping(until) = th.state {
                if now >= until || th.kill {
                    th.state = State::Ready;
                }
            }
            if th.state == State::Ready {
                next = Some(idx);
                break;
            }
        }
    }
    let Some(next) = next else { return rsp };
    if next == cur {
        return rsp;
    }
    t.current = next;
    let th = t.slots[next].as_ref().unwrap();
    if th.stack_top != 0 {
        gdt::set_kernel_stack(th.stack_top);
    }
    if Cr3::read_raw().0.start_address().as_u64() != th.cr3 {
        unsafe { asm!("mov cr3, {}", in(reg) th.cr3, options(nostack)) };
    }
    th.rsp
}

/// Let other threads run now.
pub fn yield_now() {
    unsafe { asm!("int 0x33") };
}

/// What the kernel's main loop does when it has nothing to do: give the time
/// to a program if one is waiting, otherwise rest until the next interrupt.
pub fn idle() {
    let others = {
        let t = TABLE.lock();
        let now = time::millis();
        t.slots.iter().enumerate().any(|(i, th)| {
            i != t.current
                && th.as_ref().is_some_and(|th| match th.state {
                    State::Ready => true,
                    State::Sleeping(until) => now >= until,
                    State::Dead => false,
                })
        })
    };
    if others {
        yield_now();
    } else {
        x86_64::instructions::hlt();
    }
}

/// Start a user-mode thread for process `pid` in address space `cr3`.
pub fn spawn(pid: u32, cr3: u64, entry: u64, user_rsp: u64, arg0: u64, arg1: u64) -> bool {
    let mut stack = vec![0u8; KERNEL_STACK].into_boxed_slice();
    let top = (stack.as_mut_ptr() as u64 + KERNEL_STACK as u64) & !0xF;
    let frame = top - size_of::<Regs>() as u64;
    let (cs, ss) = gdt::user_selectors();
    let regs = Regs { rdi: arg0, rsi: arg1, rip: entry, cs, rflags: 0x202, rsp: user_rsp, ss, ..Default::default() };
    unsafe { (frame as *mut Regs).write(regs) };
    let thread = Thread { pid, rsp: frame, cr3, stack_top: top, _stack: Some(stack), state: State::Ready, kill: false };

    let mut t = TABLE.lock();
    match t.slots.iter().position(|s| s.is_none()) {
        Some(i) => {
            t.slots[i] = Some(thread);
            true
        }
        None => {
            drop(t);
            drop(thread);
            false
        }
    }
}

fn with_current<R>(f: impl FnOnce(&mut Thread) -> R) -> R {
    let mut t = TABLE.lock();
    let cur = t.current;
    f(t.slots[cur].as_mut().unwrap())
}

pub fn current_pid() -> u32 {
    with_current(|th| th.pid)
}

pub fn current_cr3() -> u64 {
    with_current(|th| th.cr3)
}

/// Whether someone asked the current thread to stop.
pub fn kill_requested() -> bool {
    with_current(|th| th.kill)
}

/// Sleep the current thread for `ms` milliseconds.
pub fn sleep(ms: u64) {
    let until = time::millis().saturating_add(ms);
    with_current(|th| th.state = State::Sleeping(until));
    while time::millis() < until && !kill_requested() {
        yield_now();
    }
    with_current(|th| th.state = State::Ready);
}

/// End the current thread. Its memory is freed later by `reap`.
pub fn exit_current() -> ! {
    with_current(|th| th.state = State::Dead);
    loop {
        yield_now();
    }
}

/// Ask the threads of process `pid` to stop.
pub fn kill(pid: u32) {
    let mut t = TABLE.lock();
    for th in t.slots.iter_mut().flatten() {
        if th.pid == pid && pid != 0 {
            th.kill = true;
        }
    }
}

/// Remove finished threads. Returns their process ids.
pub fn reap() -> Vec<u32> {
    let mut dead: [Option<Thread>; MAX_THREADS] = [const { None }; MAX_THREADS];
    {
        let mut t = TABLE.lock();
        let cur = t.current;
        for (i, slot) in t.slots.iter_mut().enumerate() {
            if i != cur && slot.as_ref().is_some_and(|th| th.state == State::Dead) {
                dead[i] = slot.take();
            }
        }
    }
    dead.into_iter().flatten().map(|th| th.pid).collect()
}
