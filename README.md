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

### Using it

- **Mouse:** in QEMU the pointer follows your Mac's mouse directly, with no need to click into the window to capture it.
- **Windows:** drag a window by its title bar, click a window to bring it forward, and close it with the circle at the top right (it turns into a ×). Ctrl+W closes the focused window.
- **Top bar:** the mark on the left opens the system menu (About, Search, Restart, Shut Down). The focused app's name sits next to it. Search and the date and time are on the right.
- **Dock:** click an app to open it, or to bring it forward if it's already open. A dot marks open apps.
- **Launcher:** press Ctrl+Space, click a search icon, or just start typing on the empty desktop. Type to search, Enter to open, 1-9 to jump, Esc to close. Start with `>` to run a terminal command.
- **Apps:** terminal, notes, clock and system.
- **Terminal:** type `help`. Up and down arrows step through history, and Esc clears the line.

### Automated test

```bash
tools/qemu-test.sh "desktop: ready" /tmp/shot.png 90 type:help ret drag:300,100,600,200
```

This boots the OS with no window and waits for the given log line. Then it types, moves and clicks the mouse (see `tools/qemu-drive.py` for the actions), saves a screenshot and prints the kernel log.

## What's inside

```
boot/limine.conf        bootloader config
kernel/src/
  main.rs               boot sequence: CPU → memory → interrupts → screen → desktop
  gdt.rs, interrupts.rs CPU tables, exception handlers, timer, keyboard and mouse interrupts
  memory.rs, heap.rs    physical frame allocator, page tables, a 64 MiB kernel heap
  acpi.rs, apic.rs      ACPI MADT parsing, Local APIC timer (1000 Hz), I/O APIC routing
  keyboard.rs           PS/2 keyboard: lock-free queue filled by the interrupt, decoded outside it
  mouse.rs              PS/2 mouse, plus QEMU/VMware's absolute pointer
  time.rs               uptime ticks and the CMOS real-time clock
  gfx.rs                drawing: anti-aliased shapes, text, off-screen canvases
  screen.rs             the framebuffer
  theme.rs              the First Light palette
  boot_screen.rs        the gear, the wordmark and the ruler progress bar
  desktop.rs            the wallpaper: sea, breathing sun, drifting waves, a passing plane
  wm.rs                 window manager and compositor (redraws only what changed)
  bar.rs, dock.rs       top bar with system menu, and the app dock
  launcher.rs           search for apps and commands
  apps/                 terminal, notes, clock, system
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
6. **Desktop**: resizing windows, workspaces, saving notes once there's a disk.
7. **Windows apps**: a PE loader and a Win32 compatibility layer, the same idea as Wine, running natively on Dante.
