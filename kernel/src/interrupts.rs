//! Interrupts: CPU exceptions (crashes), the timer, the keyboard and mouse,
//! and system calls from programs.

use spin::LazyLock as Lazy;
use x86_64::{PrivilegeLevel, VirtAddr};
use x86_64::instructions::port::Port;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{apic, crash, gdt, keyboard, mouse, process, sched};

pub const TIMER_VECTOR: u8 = 0x30;
pub const KEYBOARD_VECTOR: u8 = 0x31;
pub const MOUSE_VECTOR: u8 = 0x32;

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt.divide_error.set_handler_fn(divide_error);
    idt.breakpoint.set_handler_fn(breakpoint);
    idt.invalid_opcode.set_handler_fn(invalid_opcode);
    idt.general_protection_fault.set_handler_fn(general_protection);
    idt.page_fault.set_handler_fn(page_fault);
    unsafe {
        idt.double_fault.set_handler_fn(double_fault).set_stack_index(gdt::DOUBLE_FAULT_IST);
    }
    unsafe {
        idt[TIMER_VECTOR].set_handler_addr(VirtAddr::new(sched::timer_entry as *const () as u64));
        idt[sched::YIELD_VECTOR].set_handler_addr(VirtAddr::new(sched::yield_entry as *const () as u64));
        idt[process::SYSCALL_VECTOR]
            .set_handler_addr(VirtAddr::new(process::syscall_entry as *const () as u64))
            .set_privilege_level(PrivilegeLevel::Ring3);
    }
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_irq);
    idt[MOUSE_VECTOR].set_handler_fn(mouse_irq);
    idt[apic::SPURIOUS_VECTOR].set_handler_fn(spurious);
    idt
});

pub fn init() {
    IDT.load();
}

// ---- Exceptions: in a program, the program stops; in the kernel, we stop calmly and explain ----

fn in_program(frame: &InterruptStackFrame) -> bool {
    frame.code_segment.rpl() == PrivilegeLevel::Ring3
}

extern "x86-interrupt" fn breakpoint(frame: InterruptStackFrame) {
    crate::log!("breakpoint at {:#x}", frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn divide_error(frame: InterruptStackFrame) {
    if in_program(&frame) {
        process::crashed("division by zero", Some(frame.instruction_pointer.as_u64()));
    }
    crash::stop("Division by zero", &[("at", frame.instruction_pointer.as_u64())]);
}

extern "x86-interrupt" fn invalid_opcode(frame: InterruptStackFrame) {
    if in_program(&frame) {
        process::crashed("invalid instruction", Some(frame.instruction_pointer.as_u64()));
    }
    crash::stop("Invalid instruction", &[("at", frame.instruction_pointer.as_u64())]);
}

extern "x86-interrupt" fn general_protection(frame: InterruptStackFrame, code: u64) {
    if in_program(&frame) {
        process::crashed("protection fault", Some(frame.instruction_pointer.as_u64()));
    }
    crash::stop("General protection fault", &[("at", frame.instruction_pointer.as_u64()), ("code", code)]);
}

extern "x86-interrupt" fn page_fault(frame: InterruptStackFrame, code: PageFaultErrorCode) {
    let addr = Cr2::read_raw();
    if in_program(&frame) {
        process::crashed("page fault", Some(addr));
    }
    crash::stop(
        "Page fault: touched memory that isn't mapped",
        &[("address", addr), ("at", frame.instruction_pointer.as_u64()), ("code", code.bits())],
    );
}

extern "x86-interrupt" fn double_fault(frame: InterruptStackFrame, _code: u64) -> ! {
    crash::stop("Double fault", &[("at", frame.instruction_pointer.as_u64()), ("stack", frame.stack_pointer.as_u64())]);
}

// ---- Hardware interrupts ----

extern "x86-interrupt" fn keyboard_irq(_frame: InterruptStackFrame) {
    let scancode: u8 = unsafe { Port::new(0x60).read() };
    keyboard::push_scancode(scancode);
    apic::eoi();
}

extern "x86-interrupt" fn mouse_irq(_frame: InterruptStackFrame) {
    mouse::on_irq();
    apic::eoi();
}

extern "x86-interrupt" fn spurious(_frame: InterruptStackFrame) {}
