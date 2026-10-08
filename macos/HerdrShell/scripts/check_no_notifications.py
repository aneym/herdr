#!/usr/bin/env python3
"""File-boundary scan: the Mac shell must never invoke OS notifications.

This is a source/resource integration guard, not a mocked notification test:
launching a notification to test its absence would risk the owner's desktop.
"""
import argparse
from pathlib import Path
import re
import sys

# No OS-notification API is allowed. Foundation Notification/NotificationCenter
# remains allowed: it carries in-process AppKit/window/theme lifecycle events.
# GHOSTTY_ACTION_DESKTOP_NOTIFICATION is allowed only for the exact quiet sink
# below; OSC 9/777 must be acknowledged without being forwarded to the OS.
QUIET_OSC = re.compile(
    r"if action\.tag == GHOSTTY_ACTION_DESKTOP_NOTIFICATION \{\s*"
    r"//[^\n]*\n\s*return true\s*\}"
)
# The bundled C ABI declares this action; declarations do not deliver anything.
# Allow only the enum member, not uses of that member in executable code.
ALLOW_LIST = {
    "Sources/HerdrShell/GhosttyRuntime.swift": QUIET_OSC,
    "Sources/GhosttyKit/include/ghostty.h": re.compile(
        r"(?m)^  GHOSTTY_ACTION_DESKTOP_NOTIFICATION,$"
    ),
}
FORBIDDEN = re.compile(
    r"UNUserNotificationCenter|NSUserNotification|UserNotifications|"
    r"requestUserAttention|dockTile\s*\.\s*badgeLabel|NSSound|"
    r"UN(?:MutableNotificationContent|NotificationRequest|NotificationResponse)|"
    r"requestAuthorization|notification[-_ ]?authorization|"
    r"com\.apple\.developer\.(?:usernotifications|aps)|aps-environment|"
    r"HERDR_NOTIFY|Notifier\s*\.|GHOSTTY_ACTION_DESKTOP_NOTIFICATION\b",
    re.IGNORECASE,
)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    root = parser.parse_args().root
    failures = []
    count = 0
    for directory in ("Sources", "Resources"):
        base = root / directory
        if not base.is_dir():
            failures.append(f"missing scan directory: {base}")
            continue
        for path in sorted(base.rglob("*")):
            if not path.is_file():
                continue
            # Binary resources cannot contain executable Swift/ObjC or plist paths.
            raw = path.read_bytes()
            if b"\0" in raw and path.suffix not in (".plist", ".entitlements"):
                continue
            text = raw.decode("utf-8", errors="replace")
            count += 1
            relative = path.relative_to(root).as_posix()
            allowed = ALLOW_LIST.get(relative)
            if allowed:
                # Preserve line positions for diagnostics after removing the sink.
                text = allowed.sub(lambda m: "\n" * m.group().count("\n"), text)
            for number, line in enumerate(text.splitlines(), 1):
                if FORBIDDEN.search(line):
                    failures.append(f"{relative}:{number}: {line.strip()}")
    if failures:
        print("FAIL: Mac shell OS-notification paths found", file=sys.stderr)
        print("\n".join(failures), file=sys.stderr)
        return 1
    print(f"PASS: {count} Mac Sources/Resources files contain no OS-notification paths")
    return 0


if __name__ == "__main__":
    sys.exit(main())
