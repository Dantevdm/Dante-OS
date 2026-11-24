//! Dante OS kernel.
//!
//! Limine loads us; we set up the CPU, interrupts and the timer, show the
//! First Light boot screen, and hand over to the shell.

#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

extern crate alloc;

mod acpi;
mod apic;
mod apps;
mod bar;
mod boot_screen;
mod cpu;
mod crash;
mod desktop;
mod disk;
mod dock;
mod fs;
mod gdt;
mod gfx;
mod heap;
mod interrupts;
mod keyboard;
mod launcher;
mod memory;
mod mouse;
mod pci;
mod power;
mod screen;
mod serial;
mod theme;
mod time;
mod wm;

use limine::memmap::MEMMAP_USABLE;
use limine::request::{FramebufferRequest, HhdmRequest, MemmapRequest, MpRequest, RsdpRequest};
use limine::{BaseRevision, RequestsEndMarker, RequestsStartMarker};
use spin::Mutex;

use boot_screen::BootScreen;
use core::sync::atomic::Ordering;
use screen::Screen;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The screen, shared by the kernel and the crash handler.
pub static SCREEN: Mutex<Option<Screen>> = Mutex::new(None);

// ---- Requests to the Limine bootloader (it fills in the responses before we run) ----

#[used]
#[unsafe(link_section = ".limine_requests_start")]
static REQUESTS_START: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static FRAMEBUFFER: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static MEMMAP: MemmapRequest = MemmapRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static CPUS: MpRequest = MpRequest::new(0);

#[used]
#[unsafe(link_section = ".limine_requests")]
static HHDM: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests")]
static RSDP: RsdpRequest = RsdpRequest::new();

#[used]
#[unsafe(link_section = ".limine_requests_end")]
static REQUESTS_END: RequestsEndMarker = RequestsEndMarker::new();

#[unsafe(no_mangle)]
extern "C" fn kmain() -> ! {
    serial::init();
    log!("");
    log!("Dante kernel {VERSION} starting");

    if !BASE_REVISION.is_supported() {
        log!("bootloader does not support our Limine base revision");
        halt();
    }

    gdt::init();
    interrupts::init();
    log!("cpu: descriptor tables and exception handlers ready");

    let hhdm = HHDM.response().expect("bootloader gave no direct map").offset;
    let memmap = MEMMAP.response().expect("bootloader gave no memory map").entries();
    memory::init(hhdm, memmap);
    heap::init();

    let rsdp = RSDP.response().expect("no ACPI tables").address as u64;
    let madt = acpi::find_madt(rsdp).expect("no MADT in ACPI tables");
    apic::init_local(interrupts::TIMER_VECTOR, time::HZ as u32);
    apic::init_io(&madt);
    apic::route_isa_irq(&madt, 1, interrupts::KEYBOARD_VECTOR);
    mouse::init();
    apic::route_isa_irq(&madt, 12, interrupts::MOUSE_VECTOR);
    x86_64::instructions::interrupts::enable();
    if disk::init() {
        fs::init();
    }
    log!("cpu: interrupts on");

    let usable_mib = MEMMAP
        .response()
        .map(|r| r.entries().iter().filter(|e| e.type_ == MEMMAP_USABLE).map(|e| e.length).sum::<u64>() / (1024 * 1024))
        .unwrap_or(0);
    let cores = CPUS.response().map(|r| r.cpus().len()).unwrap_or(1);
    log!("memory: {usable_mib} MiB usable");
    log!("cpu: {cores} cores online");

    let Some(fb) = FRAMEBUFFER.response().and_then(|r| r.framebuffers().first().copied()) else {
        log!("no framebuffer, staying in text-only mode");
        halt();
    };
    log!("screen: {}x{} @ {} bpp", fb.width, fb.height, fb.bpp);
    *SCREEN.lock() = Some(Screen::new(fb, theme::SKY));

    // Boot screen: about three calm seconds, paced by the timer.
    {
        let mut guard = SCREEN.lock();
        let s = guard.as_mut().unwrap();
        let boot = BootScreen::new(s);
        let (l1, l2) = boot_screen::summary(usable_mib, cores);
        boot.draw_base(s, &[l1.as_str(), l2.as_str()]);
        let start = time::millis();
        const DURATION: u64 = 3000;
        loop {
            let elapsed = time::millis() - start;
            boot.frame(s, (elapsed as f32 / DURATION as f32).min(1.0));
            if elapsed >= DURATION {
                break;
            }
            time::sleep_ms(16);
        }
        boot.ready(s);
    }
    log!("Dante kernel ready");
    apps::CORES.store(cores, Ordering::Relaxed);
    apps::DISPLAY_W.store(fb.width as usize, Ordering::Relaxed);
    apps::DISPLAY_H.store(fb.height as usize, Ordering::Relaxed);
    time::sleep_ms(400);

    let mut wm = wm::Wm::new(time::millis());
    wm.open(apps::AppKind::Terminal, time::millis());
    log!("desktop: ready");
    wm.run();
}

pub fn halt() -> ! {
    loop {
        x86_64::instructions::interrupts::disable();
        x86_64::instructions::hlt();
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    log!("KERNEL PANIC: {info}");
    let mut msg: screen::Buf<96> = screen::Buf::new();
    let _ = core::fmt::Write::write_fmt(&mut msg, format_args!("{}", info.message()));
    crash::stop(msg.as_str(), &[]);
}
