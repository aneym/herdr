"""The Mac chat contrast gate reads the Palette Swift compiles, not one inside a comment.

swift_code is a small parser (Swift's block comments nest; markers inside strings are
text), so a table of edge cases runs through low_chat_contrast against the real
ChatView.swift with one change each. The decoy cases come from the review of 3c347c6f,
where a regex stripper stopped at the first */ and the gate passed a 1.01:1 faint.
"""

import importlib.util
import pathlib
import re
import unittest

HERE = pathlib.Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("gen_tokens", HERE / "gen_tokens.py")
gen = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gen)

CHAT = gen.CHAT_VIEW.read_text()
SAFE = "var faint: Color { t.mute }"
LOW = "var faint: Color { t.mute.opacity(0.01) }"


def contrast_failures(out):
    return [line for line in out if ":1" in line]


class ChatGateTests(unittest.TestCase):
    def test_the_shipped_palette_passes(self):
        self.assertIn(SAFE, CHAT)
        self.assertEqual(gen.low_chat_contrast(gen.load(), CHAT), [])

    def test_only_the_live_faint_is_read(self):
        nested = f"/* old /* note */ {SAFE} */ {LOW}"
        cases = {
            "nested block comment hides the old faint": (nested, True),
            "line comment hides the old faint": (f"// {SAFE}\n    {LOW}", True),
            "a CR ends a line comment": (f"// old\r    {LOW}", True),
            "a declaration inside a string is text": (f'let s = "{SAFE}"\n    {LOW}', True),
            "escaped quotes stay inside the string": (f'let s = "a \\" {SAFE} \\\\"\n    {LOW}', True),
            "a raw string hides a comment opener": (f'let s = #"{SAFE} " /*"#\n    {LOW}\n    let e = "*/"', True),
            "an interpolation's string nests": (f'let s = "\\("{SAFE} /*")"\n    {LOW}\n    let e = "*/"', True),
            "a nested comment beside the live safe faint": (f"/* a /* b */ c */ {SAFE}", False),
        }
        # Outside Palette the lexer must keep string forms from swallowing the code after them.
        outside = {
            "multi-line string": 'let s = """\n    " /* \\(1)\n    """',
            "raw string with a hash escape": 'let s = #"a \\#("/*") \\" b"#',
        }
        for name, literal in outside.items():
            with self.subTest(name):
                source = CHAT.replace(SAFE, LOW, 1).replace("private struct Palette {", f"{literal}\nprivate struct Palette {{", 1)
                self.assertTrue(contrast_failures(gen.low_chat_contrast(gen.load(), source)))
        for name, (body, low) in cases.items():
            with self.subTest(name):
                out = gen.low_chat_contrast(gen.load(), CHAT.replace(SAFE, body, 1))
                if low:  # reported or refused, never passed
                    self.assertTrue(out)
                else:
                    self.assertEqual(out, [])

    def test_a_palette_the_gate_cannot_read_is_refused(self):
        cases = {
            "split live declaration": LOW.replace("var faint", "var\n    faint"),
            "regex literal holding quotes": f'let r = #/{SAFE} """/#\n    {LOW}\n    let e = #/"""/#',
        }
        for name, body in cases.items():
            with self.subTest(name):
                self.assertTrue(gen.low_chat_contrast(gen.load(), CHAT.replace(SAFE, body, 1)))

    def test_a_commented_palette_beside_a_live_one_is_refused(self):
        palette = CHAT[CHAT.index("private struct Palette {"):]
        palette = palette[:palette.index("\n}") + 2]
        live = palette.replace(SAFE, LOW).replace("private struct Palette {", "private struct Palette\n{")
        source = CHAT.replace(palette, f"/*\n/* archived palette */\n{palette}\n*/\n{live}", 1)
        self.assertNotEqual(gen.low_chat_contrast(gen.load(), source), [])


# Pane drag motion tokens (spec pane-drag-rearrange-2026-10-07, slice S2). Owner-written:
# implementers may not edit this class. It pins the motion block and checks that the four
# generated clients (Mac Swift, Windows CSS and TS, TUI Rust) carry the same numbers.
MOTION = {
    "dragThresholdPx": 4, "tabEdgePx": 12, "edgeBandMin": 24, "edgeBandFraction": 0.25,
    "edgeBandMaxFraction": 0.33, "springLoadMs": 450,
    "zoneMorphMs": 140, "settleMs": 200, "cancelMs": 160, "fadeMs": 120, "reducedFadeMs": 100,
    "ease": [0.2, 0.0, 0.0, 1.0],
    "spring": {"response": 0.28, "dampingFraction": 0.92},
    "zoneFillAlphaDark": 0.16, "zoneFillAlphaLight": 0.12, "zoneStroke": 1.5, "zoneInset": 4,
    "liftOpacity": 0.55, "chipOffset": 12,
}
PX_KEYS = {"dragThresholdPx", "tabEdgePx", "edgeBandMin", "zoneStroke", "zoneInset", "chipOffset"}
SWIFT_OUT = "macos/HerdrShell/Sources/HerdrShell/ShellTokens.swift"
CSS_OUT = "windows/HerdrShell/app/src/tokens.css"
TS_OUT = "windows/HerdrShell/app/src/tokens.ts"
RUST_OUT = "src/client/shell/motion_tokens.rs"
NUMBER = r"(-?[0-9]+(?:\.[0-9]+)?)"


def motion_scalars(motion):
    out = {k: v for k, v in motion.items() if not k.startswith("_") and k not in ("ease", "spring")}
    out["springResponse"] = motion["spring"]["response"]
    out["springDampingFraction"] = motion["spring"]["dampingFraction"]
    return out


def screaming_snake(key):
    return re.sub(r"([a-z0-9])([A-Z])", r"\1_\2", key).upper()


def block(text, opener, closer):
    start = text.index(opener)
    return text[start:text.index(closer, start)]


def number_list(text):
    return [float(v) for v in re.split(r"\s*,\s*", text.strip())]


class MotionTokenTests(unittest.TestCase):
    def setUp(self):
        self.t = gen.load()
        self.out = {path.relative_to(gen.ROOT).as_posix(): text for path, text in gen.outputs(self.t).items()}

    def test_the_motion_block_holds_the_specified_numbers(self):
        motion = {k: v for k, v in self.t["motion"].items() if not k.startswith("_")}
        self.assertEqual(motion, MOTION)

    def test_the_tui_motion_tokens_are_a_fourth_generated_output(self):
        self.assertEqual(set(self.out), {SWIFT_OUT, CSS_OUT, TS_OUT, RUST_OUT})
        self.assertIn(gen.HEADER, self.out[RUST_OUT])

    def test_every_client_reads_the_same_motion_numbers(self):
        swift = block(self.out[SWIFT_OUT], "enum ShellMotion {", "\n}")
        ts = block(self.out[TS_OUT], "export const motion = {", "\n}")
        css = self.out[CSS_OUT]
        rust = self.out[RUST_OUT]
        for key, value in motion_scalars(self.t["motion"]).items():
            ms = key.endswith("Ms")
            unit = "ms" if ms else "px" if key in PX_KEYS else ""
            patterns = {
                "swift": (swift, rf"static let {key}: CGFloat = {NUMBER}\n"),
                "css": (css, rf"--shell-motion-{gen.kebab(key)}: {NUMBER}{unit};"),
                "ts": (ts, rf"\b{key}: {NUMBER}[,\s]"),
                "rust": (rust, rf"pub const {screaming_snake(key)}: {'u64' if ms else 'f32'} = {NUMBER};"),
            }
            for client, (text, pattern) in patterns.items():
                with self.subTest(client=client, key=key):
                    match = re.search(pattern, text)
                    self.assertIsNotNone(match, f"{client} lacks {key} as {pattern}")
                    self.assertAlmostEqual(float(match.group(1)), value, places=6)
        ease = self.t["motion"]["ease"]
        eases = {
            "swift": (swift, r"static let ease: \[CGFloat\] = \[([^\]]*)\]"),
            "css": (css, r"--shell-motion-ease: cubic-bezier\(([^)]*)\);"),
            "ts": (ts, r"\bease: \[([^\]]*)\]"),
            "rust": (rust, r"pub const EASE: \[f32; 4\] = \[([^\]]*)\];"),
        }
        for client, (text, pattern) in eases.items():
            with self.subTest(client=client, key="ease"):
                match = re.search(pattern, text)
                self.assertIsNotNone(match, f"{client} lacks the ease curve as {pattern}")
                for got, want in zip(number_list(match.group(1)), ease, strict=True):
                    self.assertAlmostEqual(got, want, places=6)


# Explicit exemptions: zero is absence of layout; one logical unit is a hairline.
# Percentages and runtime geometry are not fixed design dimensions. Generated files
# are checked byte-for-byte by gen_tokens.py, not scanned as client consumers.
EXEMPT_DIMENSIONS = {0, 1}
GENERATED_DIMENSIONS = {"ShellTokens.swift", "tokens.css", "tokens.ts"}


def raw_dimensions(path, source):
    """File-boundary design contract: inspect actual shipped client consumers."""
    source = re.sub(r"/\*.*?\*/|//[^\n]*", "", source, flags=re.S)
    spans = []
    if path.suffix == ".swift":
        # Balanced padding arguments include ternaries and depth-based expressions.
        for match in re.finditer(r"\.padding\(", source):
            start = match.end()
            depth, end = 1, start
            while end < len(source) and depth:
                depth += (source[end] == "(") - (source[end] == ")")
                end += 1
            spans.append((start, source[start:end - 1]))
        for match in re.finditer(r"(?:\b(?:spacing|horizontalSpacing|verticalSpacing|cornerRadius|size|ofSize)\s*:\s*|\.cornerRadius\s*=\s*|\bspacing\s*:\s*CGFloat\s*=\s*)([^,\n)}]+)", source):
            spans.append((match.start(1), match.group(1)))
    elif path.suffix == ".css":
        for match in re.finditer(r"(?:^|(?<=[;{]))\s*(?:padding[\w-]*|margin[\w-]*|gap|row-gap|column-gap|border[\w-]*radius|font-size|letter-spacing|(?:min-|max-)?(?:width|height)|inset|top|bottom|left|right)\s*:\s*([^;}]+)", source):
            spans.append((match.start(1), match.group(1)))
    else:
        for match in re.finditer(r"\b(?:padding\w*|margin\w*|gap|rowGap|columnGap|borderRadius|fontSize)\s*:\s*([^,}\n]+)", source):
            spans.append((match.start(1), match.group(1)))
    failures = []
    for start, expression in spans:
        for number in re.finditer(r"(?<![\w.])-?\d+(?:\.\d+)?(?=px\b|[^\w.]|$)", expression):
            tail = expression[number.end():]
            before = expression[:number.start()].rstrip()
            if path.suffix == ".css" and not tail.startswith("px"):
                continue  # percentages, vh/vw and unitless layout factors
            if tail.startswith("%") or before.endswith(("==", "!=", "<=", ">=", "<", ">")):
                continue  # branch predicates are not dimension values
            if abs(float(number.group())) in EXEMPT_DIMENSIONS:
                continue
            line = source.count("\n", 0, start + number.start()) + 1
            failures.append(f"{path}:{line}: raw dimension {number.group()}")
    return failures


class ClientDimensionTests(unittest.TestCase):
    def test_literal_parser_edge_cases(self):
        """Golden syntax table for the pure literal parser's overlapping edge cases."""
        cases = [
            ("a.css", "a { gap: 7px; padding: 4px 8px; font-size: 12.5px; }", 4),
            ("a.css", "a { border-radius: 50%; padding: 0; height: 100vh; border: 1px solid; }", 0),
            ("a.css", "a { gap: var(--shell-space-step8); margin: calc(-1 * var(--shell-space-step12)); }", 0),
            ("a.tsx", 'style={{ paddingLeft: 8 + depth * 16, fontSize: "12px" }}', 3),
            ("a.swift", ".padding(.top, count > 2 ? 6 : 0).font(.system(size: 12.5))", 2),
            ("a.swift", "HStack(spacing: 4) { }.padding(.leading, CGFloat(depth) * 16)", 2),
            ("a.swift", "/* .padding(9) */ HStack(spacing: 0) { }.padding(1)", 0),
        ]
        for name, text, count in cases:
            with self.subTest(source=text):
                self.assertEqual(len(raw_dimensions(pathlib.Path(name), text)), count)

    def test_shipped_clients_use_shared_dimensions(self):
        failures = []
        for root, suffixes in ((gen.MAC_DIR, {".swift"}), (gen.WEB_DIR, {".css", ".tsx"})):
            for path in sorted(root.rglob("*")):
                if path.suffix in suffixes and path.name not in GENERATED_DIMENSIONS and not path.name.endswith(".test.tsx"):
                    failures.extend(raw_dimensions(path, path.read_text()))
        self.assertEqual(failures, [], "\n" + "\n".join(failures))


if __name__ == "__main__":
    unittest.main()
