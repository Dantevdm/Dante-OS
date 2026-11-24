#!/usr/bin/env bash
# Build the Dante kernel and boot it in QEMU.
#
#   ./run.sh            build and boot in a QEMU window (kernel log in this terminal)
#   ./run.sh --headless boot without a window; log only
#   ./run.sh --debug    wait for a debugger: gdb -ex "target remote :1234"
set -euo pipefail

ROOT="$(cd "$(dirname "$0")" && pwd)"
LIMINE_DIR="$ROOT/tools/limine"
ESP="$ROOT/build/esp"
KERNEL="$ROOT/kernel/target/x86_64-unknown-none/release/dante-kernel"

command -v qemu-system-x86_64 >/dev/null || { echo "QEMU missing: brew install qemu"; exit 1; }

# The Limine bootloader (prebuilt binaries from its official repo).
if [ ! -f "$LIMINE_DIR/BOOTX64.EFI" ]; then
    git clone --depth=1 --branch=v11.x-binary https://github.com/limine-bootloader/limine "$LIMINE_DIR"
fi

# UEFI firmware that ships with QEMU.
FIRMWARE="$(brew --prefix qemu)/share/qemu/edk2-x86_64-code.fd"
[ -f "$FIRMWARE" ] || { echo "UEFI firmware not found at $FIRMWARE"; exit 1; }

echo "==> Building kernel"
(cd "$ROOT/kernel" && cargo build --release)

echo "==> Assembling boot disk"
rm -rf "$ESP"
mkdir -p "$ESP/EFI/BOOT" "$ESP/boot/limine"
cp "$LIMINE_DIR/BOOTX64.EFI" "$ESP/EFI/BOOT/"
cp "$ROOT/boot/limine.conf" "$ESP/boot/limine/"
cp "$KERNEL" "$ESP/boot/dante-kernel"

# A virtual hard disk that keeps its contents between runs.
DISK="$ROOT/build/disk.img"
[ -f "$DISK" ] || qemu-img create -q -f raw "$DISK" 64M

QEMU_ARGS=(
    -machine q35,vmport=on
    -rtc base=localtime
    -m 2G
    -smp 4
    -drive "if=pflash,format=raw,readonly=on,file=$FIRMWARE"
    -drive "format=raw,file=fat:rw:$ESP"
    -drive "if=none,id=disk0,format=raw,file=$DISK"
    -device virtio-blk-pci,drive=disk0,disable-legacy=off,disable-modern=on
    -serial stdio
    -no-reboot
)

case "${1:-}" in
    --headless) QEMU_ARGS+=(-display none) ;;
    --debug)    QEMU_ARGS+=(-s -S) ; echo "==> Waiting for debugger on :1234" ;;
esac

echo "==> Booting Dante OS (close the window or press Ctrl+C to stop)"
exec qemu-system-x86_64 "${QEMU_ARGS[@]}"
