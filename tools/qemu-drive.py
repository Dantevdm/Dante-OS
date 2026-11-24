#!/usr/bin/env python3
"""Drive a running QEMU through its QMP socket: keys, mouse and screenshots.

    qemu-drive.py <qmp-socket> <action>...

Actions:
    h, ret, spc, ctrl-spc ...   a key (QEMU sendkey names)
    type:hello                  type text (letters, digits, space)
    move:X,Y                    move the pointer to screen pixel X,Y
    down / up / click           left mouse button
    drag:X1,Y1,X2,Y2            press at one point, move in steps, release at the other
    wait:SECONDS                pause
    shot:FILE.png               save a screenshot
"""
import json
import os
import socket
import sys
import time

W = int(os.environ.get("SCREEN_W", 1280))
H = int(os.environ.get("SCREEN_H", 800))


class Qmp:
    def __init__(self, path):
        self.s = socket.socket(socket.AF_UNIX)
        self.s.connect(path)
        self.f = self.s.makefile("rw")
        self.f.readline()  # greeting
        self.cmd("qmp_capabilities")

    def cmd(self, name, **args):
        self.f.write(json.dumps({"execute": name, "arguments": args}) + "\n")
        self.f.flush()
        while True:
            reply = json.loads(self.f.readline())
            if "return" in reply or "error" in reply:
                if "error" in reply:
                    print("qmp error:", reply["error"], file=sys.stderr)
                return reply

    def hmp(self, line):
        return self.cmd("human-monitor-command", **{"command-line": line})

    def events(self, *events):
        return self.cmd("input-send-event", events=list(events))


def abs_events(x, y):
    sx = round(x * 0x7FFF / (W - 1))
    sy = round(y * 0x7FFF / (H - 1))
    return [{"type": "abs", "data": {"axis": "x", "value": sx}}, {"type": "abs", "data": {"axis": "y", "value": sy}}]


def button(down):
    return {"type": "btn", "data": {"down": down, "button": "left"}}


KEYS = {" ": "spc", "-": "minus", ".": "dot", ">": "shift-dot", "/": "slash"}


def main():
    q = Qmp(sys.argv[1])
    for a in sys.argv[2:]:
        if a.startswith("move:"):
            x, y = map(int, a[5:].split(","))
            q.events(*abs_events(x, y))
        elif a == "down":
            q.events(button(True))
        elif a == "up":
            q.events(button(False))
        elif a == "click":
            q.events(button(True))
            time.sleep(0.08)
            q.events(button(False))
        elif a.startswith("drag:"):
            x1, y1, x2, y2 = map(int, a[5:].split(","))
            q.events(*abs_events(x1, y1))
            time.sleep(0.1)
            q.events(button(True))
            for i in range(1, 11):
                time.sleep(0.05)
                q.events(*abs_events(x1 + (x2 - x1) * i // 10, y1 + (y2 - y1) * i // 10))
            time.sleep(0.1)
            q.events(button(False))
        elif a.startswith("wait:"):
            time.sleep(float(a[5:]))
            continue
        elif a.startswith("shot:"):
            time.sleep(0.6)
            q.hmp("screendump " + a[5:] + " -f png")
            time.sleep(0.4)
            continue
        elif a.startswith("type:"):
            for ch in a[5:]:
                q.hmp("sendkey " + KEYS.get(ch, ch))
                time.sleep(0.08)
        else:
            q.hmp("sendkey " + a)
        time.sleep(0.15)


if __name__ == "__main__":
    main()
