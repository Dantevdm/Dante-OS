//! Just enough ACPI to find the interrupt controllers: walk RSDP → XSDT/RSDT →
//! MADT ("APIC" table) and read out the I/O APIC and the ISA IRQ overrides.

use crate::memory::phys_to_virt;

#[derive(Clone, Copy)]
pub struct IrqOverride {
    pub irq: u8,
    pub gsi: u32,
    pub flags: u16,
}

pub struct Madt {
    pub ioapic_addr: u64,
    pub ioapic_gsi_base: u32,
    pub overrides: [Option<IrqOverride>; 16],
}

impl Madt {
    /// The global system interrupt an old ISA IRQ (0 = timer, 1 = keyboard) arrives on.
    pub fn gsi_for_isa(&self, irq: u8) -> (u32, u16) {
        for o in self.overrides.iter().flatten() {
            if o.irq == irq {
                return (o.gsi, o.flags);
            }
        }
        (irq as u32, 0)
    }
}

unsafe fn read<T: Copy>(addr: u64) -> T {
    unsafe { (addr as *const T).read_unaligned() }
}

/// `rsdp` is the virtual address Limine gave us.
pub fn find_madt(rsdp: u64) -> Option<Madt> {
    unsafe {
        let revision: u8 = read(rsdp + 15);
        let (table_list, entry_size) = if revision >= 2 {
            (read::<u64>(rsdp + 24), 8u64)
        } else {
            (read::<u32>(rsdp + 16) as u64, 4u64)
        };
        let sdt = phys_to_virt(table_list).as_u64();
        let len: u32 = read(sdt + 4);
        let entries = (len as u64 - 36) / entry_size;

        for i in 0..entries {
            let at = sdt + 36 + i * entry_size;
            let phys = if entry_size == 8 { read::<u64>(at) } else { read::<u32>(at) as u64 };
            let table = phys_to_virt(phys).as_u64();
            if read::<[u8; 4]>(table) != *b"APIC" {
                continue;
            }
            let mut madt = Madt { ioapic_addr: 0, ioapic_gsi_base: 0, overrides: [None; 16] };
            let end = table + read::<u32>(table + 4) as u64;
            let mut p = table + 44;
            let mut n_over = 0;
            while p + 2 <= end {
                let kind: u8 = read(p);
                let size: u8 = read(p + 1);
                match kind {
                    1 if madt.ioapic_addr == 0 => {
                        madt.ioapic_addr = read::<u32>(p + 4) as u64;
                        madt.ioapic_gsi_base = read(p + 8);
                    }
                    2 if n_over < 16 => {
                        madt.overrides[n_over] = Some(IrqOverride { irq: read(p + 3), gsi: read(p + 4), flags: read(p + 8) });
                        n_over += 1;
                    }
                    _ => {}
                }
                if size < 2 {
                    break;
                }
                p += size as u64;
            }
            return Some(madt);
        }
    }
    None
}
