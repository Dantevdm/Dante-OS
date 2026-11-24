//! Interrupts: CPU exceptions (crashes), the timer and the keyboard.

use spin::LazyLock as Lazy;
use x86_64::instructions::port::Port;
use x86_64::registers::control::Cr2;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode};

use crate::{apic, crash, gdt, keyboard, time};

pub const TIMER_VECTOR: u8 = 0x30;
pub const KEYBOARD_VECTOR: u8 = 0x31;

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
    idt[TIMER_VECTOR].set_handler_fn(timer);
    idt[KEYBOARD_VECTOR].set_handler_fn(keyboard_irq);
    idt[apic::SPURIOUS_VECTOR].set_handler_fn(spurious);
    idt
});

pub fn init() {
    IDT.load();
}

// ---- Exceptions: anything here means a bug, so we stop calmly and explain ----

extern "x86-interrupt" fn breakpoint(frame: InterruptStackFrame) {
    crate::log!("breakpoint at {:#x}", frame.instruction_pointer.as_u64());
}

extern "x86-interrupt" fn divide_error(frame: InterruptStackFrame) {
    crash::stop("Division by zero", &[("at", frame.instruction_pointer.as_u64())]);
}

extern "x86-interrupt" fn invalid_opcode(frame: InterruptStackFrame) {
    crash::stop("Invalid instruction", &[("at", frame.instruction_pointer.as_u64())]);
}

extern "x86-interrupt" fn general_protection(frame: InterruptStackFrame, code: u64) {
    crash::stop("General protection fault", &[("at", frame.instruction_pointer.as_u64()), ("code", code)]);
}

extern "x86-interrupt" fn page_fault(frame: InterruptStackFrame, code: PageFaultErrorCode) {
    let addr = Cr2::read_raw();
    crash::stop(
        "Page fault: touched memory that isn't mapped",
        &[("address", addr), ("at", frame.instruction_pointer.as_u64()), ("code", code.bits())],
    );
}

extern "x86-interrupt" fn double_fault(frame: InterruptStackFrame, _code: u64) -> ! {
    crash::stop("Double fault", &[("at", frame.instruction_pointer.as_u64()), ("stack", frame.stack_pointer.as_u64())]);
}

// ---- Hardware interrupts ----

extern "x86-interrupt" fn timer(_frame: InterruptStackFrame) {
    time::tick();
    apic::eoi();
}

extern "x86-interrupt" fn keyboard_irq(_frame: InterruptStackFrame) {
    let scancode: u8 = unsafe { Port::new(0x60).read() };
    keyboard::push_scancode(scancode);
    apic::eoi();
}

extern "x86-interrupt" fn spurious(_frame: InterruptStackFrame) {}
