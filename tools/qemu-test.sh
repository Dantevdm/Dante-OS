#!/usr/bin/env bash
# Headless test boot: builds, boots in QEMU without a window, waits for a line
# in the kernel log, optionally types keys, then saves a screenshot.
#
#   tools/qemu-test.sh <wait-for-text> <out.png> [timeout-seconds] [keys...]
#
# Keys use QEMU's sendkey names, e.g. h e l p ret.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WAIT_FOR="$1"; OUT="$2"; TIMEOUT="${3:-60}"; shift 3 || true
ESP="$ROOT/build/esp"
LOG="$ROOT/build/serial.log"
SOCK="$ROOT/build/m.sock"
FW="$(brew --prefix qemu)/share/qemu/edk2-x86_64-code.fd"

(cd "$ROOT/kernel" && cargo build --release 2>&1 | grep -E "^(warning|error)" -A5 || true)
rm -rf "$ESP"; mkdir -p "$ESP/EFI/BOOT" "$ESP/boot/limine"
cp "$ROOT/tools/limine/BOOTX64.EFI" "$ESP/EFI/BOOT/"
cp "$ROOT/boot/limine.conf" "$ESP/boot/limine/"
cp "$ROOT/kernel/target/x86_64-unknown-none/release/dante-kernel" "$ESP/boot/dante-kernel"
rm -f "$LOG" "$SOCK"

qemu-system-x86_64 -machine q35 -m 2G -smp 4 \
    -drive "if=pflash,format=raw,readonly=on,file=$FW" \
    -drive "format=raw,file=fat:rw:$ESP" \
    -serial "file:$LOG" -display none -no-reboot \
    -monitor "unix:$SOCK,server,nowait" &
QPID=$!
trap 'kill $QPID 2>/dev/null; rm -f "$SOCK"' EXIT

found=0
for _ in $(seq 1 "$TIMEOUT"); do
    if grep -q "$WAIT_FOR" "$LOG" 2>/dev/null; then found=1; break; fi
    if ! kill -0 $QPID 2>/dev/null; then break; fi
    sleep 1
done

mon() { echo "$1" | nc -U -w 1 "$SOCK" >/dev/null 2>&1 || true; }
for k in "$@"; do mon "sendkey $k"; sleep 0.15; done
[ $# -gt 0 ] && sleep 2
mon "screendump $OUT -f png"
sleep 1

echo "---- kernel log ----"
tr -d '\r' < "$LOG" | sed -n '/Dante kernel/,$p'
[ $found = 1 ] && echo "---- found: $WAIT_FOR ----" || { echo "---- NOT FOUND: $WAIT_FOR ----"; exit 1; }
