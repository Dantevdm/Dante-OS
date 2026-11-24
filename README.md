# Dante OS

A small operating system written from scratch in Rust. It has its own kernel, and its look is called **First Light**: a still sea at dawn, drawn like a technical drawing.

## Run it

You need [Rust](https://rustup.rs) and QEMU (`brew install qemu`). The right nightly toolchain installs itself from `kernel/rust-toolchain.toml`.

```bash
./run.sh
```

- `./run.sh --headless` runs with no window; you see only the kernel log.
- `./run.sh --debug` waits for a debugger: `gdb -ex "target remote :1234"`.

The first run downloads the prebuilt Limine bootloader into `tools/limine`.

At the shell, type `help`. The commands are `about`, `mem`, `cpu`, `uptime`, `time`, `echo`, `clear`, `crash`, `reboot` and `shutdown`. The up and down arrows step through history, and Esc clears the line.

### Automated test

```bash
tools/qemu-test.sh "shell: ready" /tmp/shot.png 90 h e l p ret
```

This boots the OS with no window, waits for the given log line, types the keys, saves a screenshot and prints the kernel log.

## What's inside

```
boot/limine.conf        bootloader config
kernel/src/
  main.rs               boot sequence: CPU → memory → interrupts → screen → shell
  gdt.rs, interrupts.rs CPU tables, exception handlers, the timer and keyboard interrupts
  memory.rs, heap.rs    physical frame allocator, page tables, a 16 MiB kernel heap
  acpi.rs, apic.rs      ACPI MADT parsing, Local APIC timer (1000 Hz), I/O APIC routing
  keyboard.rs           PS/2 keyboard: lock-free queue filled by the interrupt, decoded outside it
  time.rs               uptime ticks and the CMOS real-time clock
  screen.rs             framebuffer drawing: gradients, anti-aliased lines and discs, text
  theme.rs              the First Light palette
  boot_screen.rs        the gear, the wordmark and the ruler progress bar
  desktop.rs, shell.rs  the desktop (sea, sun, header) and the terminal window
  crash.rs              a calm crash page instead of a frozen screen
Design/                 the design canvas (all rounds, including First Light)
tools/qemu-test.sh      headless boot + keystrokes + screenshot
```

## Roadmap

1. **All cores**: start the other CPUs (SMP) and give each one its own scheduler queue.
2. **Tasks**: kernel threads, then preemptive scheduling on the APIC timer.
3. **User space**: ring 3, system calls, and loading ELF programs.
4. **Storage**: a virtio-blk or NVMe driver and a simple filesystem.
5. **Real power-off**: ACPI shutdown and reboot, read from the FADT and DSDT.
6. **Input and graphics**: mouse, double-buffered compositing, more than one window.
7. **Windows apps**: a PE loader and a Win32 compatibility layer, the same idea as Wine, running natively on Dante.
