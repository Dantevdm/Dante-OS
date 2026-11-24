#!/usr/bin/env bash
# Headless test boot: builds, boots in QEMU without a window, waits for a line
# in the kernel log, then performs actions (keys, mouse, screenshots).
#
#   tools/qemu-test.sh <wait-for-text> <out.png> [timeout-seconds] [actions...]
#
# Actions are described in tools/qemu-drive.py, for example:
#   type:help ret  move:640,400 click  drag:300,100,600,200  shot:/tmp/a.png
# A final screenshot is always saved to <out.png>.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WAIT_FOR="$1"; OUT="$2"; TIMEOUT="${3:-60}"; shift 3 || true
ESP="$ROOT/build/esp"
LOG="$ROOT/build/serial.log"
SOCK="$ROOT/build/q.sock"
FW="$(brew --prefix qemu)/share/qemu/edk2-x86_64-code.fd"

(cd "$ROOT/kernel" && cargo build --release 2>&1 | grep -E "^(warning|error)" -A5 || true)
rm -rf "$ESP"; mkdir -p "$ESP/EFI/BOOT" "$ESP/boot/limine"
cp "$ROOT/tools/limine/BOOTX64.EFI" "$ESP/EFI/BOOT/"
cp "$ROOT/boot/limine.conf" "$ESP/boot/limine/"
cp "$ROOT/kernel/target/x86_64-unknown-none/release/dante-kernel" "$ESP/boot/dante-kernel"
rm -f "$LOG" "$SOCK"
# A fresh disk each run, unless KEEP_DISK=1 (to test that files survive a reboot).
DISK="$ROOT/build/test-disk.img"
[ "${KEEP_DISK:-0}" = 1 ] && [ -f "$DISK" ] || { rm -f "$DISK"; qemu-img create -q -f raw "$DISK" 64M; }

qemu-system-x86_64 -machine q35,vmport=on -rtc base=localtime -m 2G -smp 4 \
    -drive "if=pflash,format=raw,readonly=on,file=$FW" \
    -drive "format=raw,file=fat:rw:$ESP" \
    -drive "if=none,id=disk0,format=raw,file=$DISK" \
    -device virtio-blk-pci,drive=disk0,disable-legacy=off,disable-modern=on \
    -serial "file:$LOG" -display none -no-reboot \
    -qmp "unix:$SOCK,server,nowait" &
QPID=$!
trap 'kill $QPID 2>/dev/null; rm -f "$SOCK"' EXIT

found=0
for _ in $(seq 1 "$TIMEOUT"); do
    if grep -q "$WAIT_FOR" "$LOG" 2>/dev/null; then found=1; break; fi
    if ! kill -0 $QPID 2>/dev/null; then break; fi
    sleep 1
done

if [ $found = 1 ]; then
    sleep 1
    python3 "$ROOT/tools/qemu-drive.py" "$SOCK" "$@" "wait:1.5" "shot:$OUT" || true
fi

echo "---- kernel log ----"
tr -d '\r' < "$LOG" | sed -n '/Dante kernel/,$p'
[ $found = 1 ] && echo "---- found: $WAIT_FOR ----" || { echo "---- NOT FOUND: $WAIT_FOR ----"; exit 1; }
