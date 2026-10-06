# wheellog.py OUT: SGR mouse reporting on; every wheel report goes to OUT as "ms<TAB>button".
# Quits when OUT.stop exists.
import os, re, select, sys, termios, time, tty

out = sys.argv[1]
fd = sys.stdin.fileno()
old = termios.tcgetattr(fd)
tty.setraw(fd)
os.write(1, b"\x1b[?1000h\x1b[?1006h\x1b[2J\x1b[Hwheellog ready\r\n")
buf, t0, rows = b"", None, []
try:
    while not os.path.exists(out + ".stop"):
        r, _, _ = select.select([fd], [], [], 0.2)
        if not r:
            continue
        buf += os.read(fd, 4096)
        now = time.monotonic()
        last = 0
        for m in re.finditer(rb"\x1b\[<(\d+);\d+;\d+[Mm]", buf):
            b = int(m.group(1))
            last = m.end()
            if b & 64:
                t0 = t0 or now
                rows.append(((now - t0) * 1000, b))
        buf = buf[last:][-64:]
        with open(out, "w") as f:
            f.write("".join(f"{t:.1f}\t{b}\n" for t, b in rows))
finally:
    os.write(1, b"\x1b[?1000l\x1b[?1006l")
    termios.tcsetattr(fd, termios.TCSADRAIN, old)
