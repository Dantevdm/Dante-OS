//! Processes: programs loaded from the disk, each in its own address space,
//! running in user mode (ring 3) and talking to the kernel through system
//! calls (`int 0x80`).
//!
//! System calls: the number goes in rax, arguments in rdi, rsi, rdx and r10,
//! and the result comes back in rax. A result from -1 to -4095 is an error
//! (see `Errno`). File descriptors 0, 1 and 2 are the terminal.
//!
//!    0 exit(code)                       end the program
//!    1 write(fd, ptr, len) -> n         write to the terminal or a file
//!    2 sleep(ms)                        pause
//!    3 millis() -> ms                   milliseconds since start
//!    4 yield()                          let others run
//!    5 read(fd, ptr, len) -> n          read typed input or a file; 0 at the end
//!    6 open(path, len, mode) -> fd      mode 0 read, 1 write (replace), 2 append
//!    7 close(fd)                        close a file, saving what was written
//!    8 list(path, len, buf, size) -> n  folder entries, one per line, folders end in '/';
//!                                       n is the full size, written only if it fits
//!    9 grow(bytes) -> address           more memory for the program
//!   10 remove(path, len)                delete a file or empty folder
//!   11 mkdir(path, len)                 make a folder
//!
//! Paths are relative to the folder the program was started in.

use alloc::format;
use alloc::string::{String, ToString};
use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};
use spin::Mutex;
use x86_64::instructions::interrupts;

use crate::memory::{self, PAGE, USER_TOP};
use crate::sched::{self, Regs, entry};
use crate::{fs, time};

pub const SYSCALL_VECTOR: u8 = 0x80;

const SYS_EXIT: u64 = 0;
const SYS_WRITE: u64 = 1;
const SYS_SLEEP: u64 = 2;
const SYS_MILLIS: u64 = 3;
const SYS_YIELD: u64 = 4;
const SYS_READ: u64 = 5;
const SYS_OPEN: u64 = 6;
const SYS_CLOSE: u64 = 7;
const SYS_LIST: u64 = 8;
const SYS_GROW: u64 = 9;
const SYS_REMOVE: u64 = 10;
const SYS_MKDIR: u64 = 11;

const STACK_TOP: u64 = 0x0000_7FFF_FFFF_0000;
const STACK_SIZE: u64 = 64 * 1024;
const MAX_ARGS: usize = 1024;
const MAX_OUTPUT: usize = 64 * 1024;
const MAX_FILES: usize = 16;
const MAX_PATH: u64 = 1024;
const HEAP_BASE: u64 = 0x0000_0010_0000_0000;
const HEAP_MAX: u64 = 256 * 1024 * 1024;

/// Error results, returned as negative numbers.
#[derive(Clone, Copy)]
enum Errno {
    Invalid = 1,
    NotFound = 2,
    Exists = 3,
    NotDir = 4,
    IsDir = 5,
    NotEmpty = 6,
    NoSpace = 7,
    TooBig = 8,
    BadName = 9,
    Io = 10,
    NoMemory = 11,
    TooMany = 12,
}

impl Errno {
    fn ret(self) -> u64 {
        (-(self as i64)) as u64
    }
}

impl From<fs::Error> for Errno {
    fn from(e: fs::Error) -> Self {
        match e {
            fs::Error::NotFound => Errno::NotFound,
            fs::Error::Exists => Errno::Exists,
            fs::Error::NotDir => Errno::NotDir,
            fs::Error::IsDir => Errno::IsDir,
            fs::Error::NotEmpty => Errno::NotEmpty,
            fs::Error::NoSpace => Errno::NoSpace,
            fs::Error::TooBig => Errno::TooBig,
            fs::Error::BadName => Errno::BadName,
            fs::Error::NoDisk | fs::Error::Io => Errno::Io,
        }
    }
}

type SysResult = Result<u64, Errno>;

/// A file a program has open. Its contents are held in memory and written
/// back to the disk when it is closed (or when the program ends).
struct OpenFile {
    path: String,
    data: Vec<u8>,
    pos: usize,
    writable: bool,
    dirty: bool,
}

pub enum Status {
    Exited(i64),
    Killed,
    Crashed(String),
}

struct Proc {
    pid: u32,
    name: String,
    l4: u64,
    output: Vec<u8>,
    status: Option<Status>,
    finished: bool,
    detached: bool,
    cwd: String,
    input: VecDeque<u8>,
    input_closed: bool,
    files: Vec<Option<OpenFile>>,
    brk: u64,
}

static PROCS: Mutex<Vec<Proc>> = Mutex::new(Vec::new());
static NEXT_PID: AtomicU32 = AtomicU32::new(1);

entry!(syscall_entry, on_syscall);

/// Load the program at `path` and start it. Returns its process id.
pub fn spawn(path: &str, args: &str, cwd: &str) -> Result<u32, String> {
    let data = fs::read(path).map_err(|e| e.text().to_string())?;
    if args.len() > MAX_ARGS {
        return Err("arguments too long".into());
    }
    let l4 = memory::new_address_space().ok_or("out of memory")?;
    let fail = |e: &str| {
        memory::free_address_space(l4);
        Err(e.to_string())
    };
    let entry = match load_elf(l4, &data) {
        Ok(e) => e,
        Err(e) => return fail(e),
    };
    for page in (STACK_TOP - STACK_SIZE..STACK_TOP).step_by(PAGE as usize) {
        if memory::map_user(l4, page, true, false).is_none() {
            return fail("out of memory");
        }
    }
    // The arguments sit at the top of the stack; main gets a pointer and a length.
    let args_at = (STACK_TOP - args.len() as u64 - 16) & !0xF;
    memory::copy_to_user(l4, args_at, args.as_bytes());
    let rsp = args_at - 8;

    let pid = NEXT_PID.fetch_add(1, Ordering::Relaxed);
    let name = path.rsplit('/').next().unwrap_or(path).to_string();
    PROCS.lock().push(Proc {
        pid,
        name,
        l4,
        output: Vec::new(),
        status: None,
        finished: false,
        detached: false,
        cwd: cwd.to_string(),
        input: VecDeque::new(),
        input_closed: false,
        files: Vec::new(),
        brk: HEAP_BASE,
    });
    if !sched::spawn(pid, l4, entry, rsp, args_at, args.len() as u64) {
        PROCS.lock().retain(|p| p.pid != pid);
        return fail("too many programs running");
    }
    crate::log!("process: {pid} started {path}");
    Ok(pid)
}

/// Copy the program's loadable segments into the address space. Returns the entry point.
fn load_elf(l4: u64, d: &[u8]) -> Result<u64, &'static str> {
    let u16_at = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let u64_at = |o: usize| d.get(o..o + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    const NOT_PROGRAM: &str = "not a program";

    if d.len() < 64 || &d[..4] != b"\x7FELF" || d[4] != 2 || d[5] != 1 {
        return Err(NOT_PROGRAM);
    }
    if u16_at(16) != Some(2) || u16_at(18) != Some(0x3E) {
        return Err("not an x86-64 program");
    }
    let entry = u64_at(24).ok_or(NOT_PROGRAM)?;
    let phoff = u64_at(32).ok_or(NOT_PROGRAM)? as usize;
    let phsize = u16_at(54).ok_or(NOT_PROGRAM)? as usize;
    let phnum = u16_at(56).ok_or(NOT_PROGRAM)? as usize;

    for i in 0..phnum {
        let h = phoff + i * phsize;
        if u32_at(h) != Some(1) {
            continue; // only PT_LOAD segments are loaded
        }
        let flags = u32_at(h + 4).ok_or(NOT_PROGRAM)?;
        let offset = u64_at(h + 8).ok_or(NOT_PROGRAM)? as usize;
        let vaddr = u64_at(h + 16).ok_or(NOT_PROGRAM)?;
        let filesz = u64_at(h + 32).ok_or(NOT_PROGRAM)?;
        let memsz = u64_at(h + 40).ok_or(NOT_PROGRAM)?;
        let end = vaddr.checked_add(memsz).ok_or(NOT_PROGRAM)?;
        if vaddr < PAGE || end > STACK_TOP - STACK_SIZE || filesz > memsz || offset + filesz as usize > d.len() {
            return Err(NOT_PROGRAM);
        }
        let file = &d[offset..offset + filesz as usize];
        for page in (vaddr / PAGE * PAGE..end).step_by(PAGE as usize) {
            let p = memory::map_user(l4, page, flags & 2 != 0, flags & 1 != 0).ok_or("out of memory")?;
            let lo = page.max(vaddr);
            let hi = (page + PAGE).min(vaddr + filesz);
            if hi > lo {
                let src = &file[(lo - vaddr) as usize..(hi - vaddr) as usize];
                unsafe { core::ptr::copy_nonoverlapping(src.as_ptr(), p.add((lo - page) as usize), src.len()) };
            }
        }
    }
    if entry >= USER_TOP {
        return Err(NOT_PROGRAM);
    }
    Ok(entry)
}

// ---- System calls (run on the program's kernel stack, with interrupts on) ----

extern "C" fn on_syscall(rsp: u64) -> u64 {
    let regs = unsafe { &mut *(rsp as *mut Regs) };
    interrupts::enable();
    let (a, b, c, d) = (regs.rdi, regs.rsi, regs.rdx, regs.r10);
    let result = match regs.rax {
        SYS_EXIT => finish(Status::Exited(a as i64)),
        SYS_WRITE => write(a, b, c),
        SYS_SLEEP => {
            sched::sleep(a);
            Ok(0)
        }
        SYS_MILLIS => Ok(time::millis()),
        SYS_YIELD => {
            sched::yield_now();
            Ok(0)
        }
        SYS_READ => read(a, b, c),
        SYS_OPEN => open(a, b, c),
        SYS_CLOSE => close(a),
        SYS_LIST => list_folder(a, b, c, d),
        SYS_GROW => grow(a),
        SYS_REMOVE => user_path(a, b).and_then(|p| fs::remove(&p).map_err(Errno::from)).map(|_| 0),
        SYS_MKDIR => user_path(a, b).and_then(|p| fs::mkdir(&p).map_err(Errno::from)).map(|_| 0),
        _ => Err(Errno::Invalid),
    };
    if sched::kill_requested() {
        finish(Status::Killed);
    }
    interrupts::disable();
    regs.rax = result.unwrap_or_else(Errno::ret);
    rsp
}

/// Run `f` on the current program's record.
fn with_proc<R>(f: impl FnOnce(&mut Proc) -> R) -> Result<R, Errno> {
    let pid = sched::current_pid();
    let mut procs = PROCS.lock();
    let p = procs.iter_mut().find(|p| p.pid == pid).ok_or(Errno::Invalid)?;
    Ok(f(p))
}

/// The program's memory at [ptr, ptr + len), checked.
fn user_slice<'a>(ptr: u64, len: u64) -> Result<&'a [u8], Errno> {
    if !memory::is_user_range(sched::current_cr3(), ptr, len, false) {
        return Err(Errno::Invalid);
    }
    Ok(unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) })
}

fn user_slice_mut<'a>(ptr: u64, len: u64) -> Result<&'a mut [u8], Errno> {
    if !memory::is_user_range(sched::current_cr3(), ptr, len, true) {
        return Err(Errno::Invalid);
    }
    Ok(unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, len as usize) })
}

/// A path from the program, made absolute.
fn user_path(ptr: u64, len: u64) -> Result<String, Errno> {
    if len == 0 || len > MAX_PATH {
        return Err(Errno::BadName);
    }
    let path = core::str::from_utf8(user_slice(ptr, len)?).map_err(|_| Errno::BadName)?;
    with_proc(|p| fs::resolve(&p.cwd, path))
}

fn write(fd: u64, ptr: u64, len: u64) -> SysResult {
    let bytes = user_slice(ptr, len)?;
    if fd == 1 || fd == 2 {
        let bytes = bytes.to_vec();
        loop {
            // If nobody is reading, wait rather than fill memory.
            let done = with_proc(|p| {
                let fits = p.output.is_empty() || p.output.len() + bytes.len() <= MAX_OUTPUT;
                if fits {
                    p.output.extend_from_slice(&bytes);
                }
                fits
            })?;
            if done {
                return Ok(len);
            }
            if sched::kill_requested() {
                return Err(Errno::Invalid);
            }
            sched::sleep(5);
        }
    }
    with_proc(|p| {
        let f = file(p, fd)?;
        if !f.writable {
            return Err(Errno::Invalid);
        }
        let end = f.pos + bytes.len();
        if end > f.data.len() {
            f.data.resize(end, 0);
        }
        f.data[f.pos..end].copy_from_slice(bytes);
        f.pos = end;
        f.dirty = true;
        Ok(len)
    })?
}

fn read(fd: u64, ptr: u64, len: u64) -> SysResult {
    let buf = user_slice_mut(ptr, len)?;
    if fd == 0 {
        // Typed input, a line at a time from the terminal. Waits for it.
        loop {
            let got = with_proc(|p| {
                if p.input.is_empty() {
                    return if p.input_closed { Some(0) } else { None };
                }
                let mut n = 0;
                while n < buf.len() {
                    let Some(b) = p.input.pop_front() else { break };
                    buf[n] = b;
                    n += 1;
                    if b == b'\n' {
                        break;
                    }
                }
                Some(n)
            })?;
            if let Some(n) = got {
                return Ok(n as u64);
            }
            if sched::kill_requested() {
                return Err(Errno::Invalid);
            }
            sched::sleep(10);
        }
    }
    with_proc(|p| {
        let f = file(p, fd)?;
        let n = buf.len().min(f.data.len().saturating_sub(f.pos));
        buf[..n].copy_from_slice(&f.data[f.pos..f.pos + n]);
        f.pos += n;
        Ok(n as u64)
    })?
}

fn file(p: &mut Proc, fd: u64) -> Result<&mut OpenFile, Errno> {
    let i = (fd as usize).checked_sub(3).ok_or(Errno::Invalid)?;
    p.files.get_mut(i).and_then(|f| f.as_mut()).ok_or(Errno::Invalid)
}

fn open(ptr: u64, len: u64, mode: u64) -> SysResult {
    let path = user_path(ptr, len)?;
    let data = match mode {
        0 => fs::read(&path)?,
        1 => {
            fs::write(&path, &[])?;
            Vec::new()
        }
        2 => match fs::read(&path) {
            Ok(d) => d,
            Err(fs::Error::NotFound) => {
                fs::write(&path, &[])?;
                Vec::new()
            }
            Err(e) => return Err(e.into()),
        },
        _ => return Err(Errno::Invalid),
    };
    let pos = if mode == 2 { data.len() } else { 0 };
    let f = OpenFile { path, data, pos, writable: mode != 0, dirty: false };
    with_proc(|p| {
        let slot = match p.files.iter().position(|f| f.is_none()) {
            Some(i) => i,
            None if p.files.len() < MAX_FILES => {
                p.files.push(None);
                p.files.len() - 1
            }
            None => return Err(Errno::TooMany),
        };
        p.files[slot] = Some(f);
        Ok(slot as u64 + 3)
    })?
}

fn close(fd: u64) -> SysResult {
    let f = with_proc(|p| {
        file(p, fd)?;
        Ok::<_, Errno>(p.files[fd as usize - 3].take().unwrap())
    })??;
    save(&f).map(|_| 0)
}

fn save(f: &OpenFile) -> Result<(), Errno> {
    if f.dirty {
        fs::write(&f.path, &f.data)?;
    }
    Ok(())
}

fn list_folder(ptr: u64, len: u64, buf: u64, size: u64) -> SysResult {
    let path = user_path(ptr, len)?;
    let mut text = String::new();
    for e in fs::list(&path)? {
        text.push_str(&e.name);
        if e.dir {
            text.push('/');
        }
        text.push('\n');
    }
    if text.len() as u64 <= size {
        user_slice_mut(buf, text.len() as u64)?.copy_from_slice(text.as_bytes());
    }
    Ok(text.len() as u64)
}

fn grow(bytes: u64) -> SysResult {
    let bytes = bytes.div_ceil(PAGE) * PAGE;
    with_proc(|p| {
        let old = p.brk;
        if old - HEAP_BASE + bytes > HEAP_MAX {
            return Err(Errno::NoMemory);
        }
        for page in (old..old + bytes).step_by(PAGE as usize) {
            memory::map_user(p.l4, page, true, false).ok_or(Errno::NoMemory)?;
        }
        p.brk = old + bytes;
        Ok(old)
    })?
}

/// Record how the current program ended, and end it.
fn finish(status: Status) -> ! {
    let pid = sched::current_pid();
    if let Some(p) = PROCS.lock().iter_mut().find(|p| p.pid == pid) {
        p.status.get_or_insert(status);
    }
    sched::exit_current()
}

/// Called from the CPU's exception handlers when a program does something
/// it isn't allowed to: the program stops, the computer carries on.
pub fn crashed(what: &'static str, addr: Option<u64>) -> ! {
    interrupts::enable();
    let text = match addr {
        Some(a) => format!("{what} at {a:#x}"),
        None => what.to_string(),
    };
    finish(Status::Crashed(text))
}

// ---- For the kernel ----

/// Free finished programs. Called often by the desktop.
pub fn reap() {
    for pid in sched::reap() {
        let mut procs = PROCS.lock();
        let Some(i) = procs.iter().position(|p| p.pid == pid) else { continue };
        let p = &mut procs[i];
        for f in p.files.drain(..).flatten() {
            if let Err(e) = save(&f) {
                crate::log!("process: {pid} could not save {}: {}", f.path, e as i64);
            }
        }
        memory::free_address_space(p.l4);
        p.finished = true;
        let how = match p.status.get_or_insert(Status::Killed) {
            Status::Exited(code) => format!("exited with {code}"),
            Status::Killed => "stopped".into(),
            Status::Crashed(what) => format!("crashed: {what}"),
        };
        crate::log!("process: {pid} {} {how}", p.name);
        if p.detached {
            procs.remove(i);
        }
    }
}

/// New output from a program, and how it ended once it has.
pub fn poll(pid: u32) -> Option<(Vec<u8>, Option<Status>)> {
    let mut procs = PROCS.lock();
    let i = procs.iter().position(|p| p.pid == pid)?;
    let output = core::mem::take(&mut procs[i].output);
    if procs[i].finished {
        let p = procs.remove(i);
        return Some((output, p.status));
    }
    Some((output, None))
}

pub fn kill(pid: u32) -> bool {
    let found = PROCS.lock().iter().any(|p| p.pid == pid && !p.finished);
    if found {
        sched::kill(pid);
    }
    found
}

/// Stop a program nobody will read from again.
pub fn detach(pid: u32) {
    let mut procs = PROCS.lock();
    if let Some(i) = procs.iter().position(|p| p.pid == pid) {
        if procs[i].finished {
            procs.remove(i);
        } else {
            procs[i].detached = true;
            procs[i].output = Vec::new();
        }
    }
    drop(procs);
    sched::kill(pid);
}

/// Typed input for a program, from the terminal.
pub fn send_input(pid: u32, bytes: &[u8]) {
    if let Some(p) = PROCS.lock().iter_mut().find(|p| p.pid == pid) {
        p.input.extend(bytes);
    }
}

/// No more typed input (Ctrl+D): reads return 0 once the rest is used up.
pub fn close_input(pid: u32) {
    if let Some(p) = PROCS.lock().iter_mut().find(|p| p.pid == pid) {
        p.input_closed = true;
    }
}

/// Running programs: (pid, name).
pub fn list() -> Vec<(u32, String)> {
    PROCS.lock().iter().filter(|p| !p.finished).map(|p| (p.pid, p.name.clone())).collect()
}
