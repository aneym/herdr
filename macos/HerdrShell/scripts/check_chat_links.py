#!/usr/bin/env python3
"""Pure parser edge-case table: exact link targets, markdown preservation and display wrapping.

The app cannot be launched on the host. This subprocess check compiles the production
linkifier and exercises its pure algorithm; existing checks do not cover chat links.
No test-only production seams or UI/source-string assertions. Writes checks/CHAT-LINKS.txt.
"""
import json
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parents[1]
LONG = "https://example.com/" + "a" * 300 + "?key=value&other=end"
CASES = [
    ("bare URL in prose", "Visit https://example.com/path today", False, ["https://example.com/path"]),
    ("markdown target preserved", "[https://display.example](https://target.example/path)", True, ["https://target.example/path"]),
    ("trailing period trimmed", "See https://example.com/path.", False, ["https://example.com/path"]),
    ("unbalanced paren trimmed", "(https://example.com/path)", False, ["https://example.com/path"]),
    ("balanced paren retained", "https://example.com/a(b).", False, ["https://example.com/a(b)"]),
    ("trailing punctuation trimmed", "https://example.com/path.,;:!?)]}>'\"", False, ["https://example.com/path"]),
    ("long URL exact target and display-only breaks", LONG, False, [LONG]),
    ("non-http schemes not autolinked", "javascript:alert(1) ftp://example.com/path mailto:a@example.com", False, []),
    ("no URL unchanged", "Plain text with emoji 🙂 and accents café", False, []),
    ("assistant bare URL after markdown parsing", "**Visit** https://example.com/path", True, ["https://example.com/path"]),
    ("multiple URLs and Unicode offsets", "🙂 https://one.example/x, then https://two.example/y!", False, ["https://one.example/x", "https://two.example/y"]),
    ("long markdown label keeps its target", f"[{LONG}](https://target.example/)", True, ["https://target.example/"]),
]
lines = []
with tempfile.TemporaryDirectory(prefix="chat-links-") as tmp:
    binary = pathlib.Path(tmp) / "chat_links_dump"
    subprocess.run(["swiftc", "-module-cache-path", str(pathlib.Path(tmp) / "modules"),
                    str(ROOT / "Sources/HerdrShell/ChatLinks.swift"),
                    str(ROOT / "scripts/chat_links_dump.swift"), "-o", str(binary)], check=True)
    outputs = json.loads(subprocess.check_output([str(binary)], input=json.dumps([
        {"text": text, "markdown": markdown} for _, text, markdown, _ in CASES
    ]), text=True))
for (name, text, markdown, targets), output in zip(CASES, outputs):
    ok = [link["target"] for link in output["links"]] == targets
    ok &= all("\u200b" not in link["target"] for link in output["links"])
    if "long" in name:
        ok &= "\u200b" in output["display"]
    if not markdown:
        ok &= output["display"].replace("\u200b", "") == text
    line = f"[{'PASS' if ok else 'FAIL'}] {name}"
    lines.append(line)
    print(line)
(ROOT / "checks").mkdir(exist_ok=True)
(ROOT / "checks/CHAT-LINKS.txt").write_text("\n".join(lines) + "\n")
raise SystemExit(0 if len(outputs) == len(CASES) and all(line.startswith("[PASS]") for line in lines) else 1)
