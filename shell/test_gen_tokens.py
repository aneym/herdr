"""The Mac chat contrast gate reads the Palette Swift compiles, not one inside a comment.

swift_code is a small parser (Swift's block comments nest; markers inside strings are
text), so a table of edge cases runs through low_chat_contrast against the real
ChatView.swift with one change each. The decoy cases come from the review of 3c347c6f,
where a regex stripper stopped at the first */ and the gate passed a 1.01:1 faint.
"""

import importlib.util
import pathlib
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
            "comment markers in a string stay text": (f'let s = "/* */ //"\n    {LOW}', True),
            "a nested comment around a live safe faint's neighbour": (f"/* a /* b */ c */ {SAFE}", False),
        }
        for name, (body, low) in cases.items():
            with self.subTest(name):
                out = gen.low_chat_contrast(gen.load(), CHAT.replace(SAFE, body, 1))
                if low:
                    self.assertTrue(contrast_failures(out), out)
                else:
                    self.assertEqual(out, [])

    def test_a_commented_palette_beside_a_live_one_is_refused(self):
        palette = CHAT[CHAT.index("private struct Palette {"):]
        palette = palette[:palette.index("\n}") + 2]
        live = palette.replace(SAFE, LOW).replace("private struct Palette {", "private struct Palette\n{")
        source = CHAT.replace(palette, f"/*\n/* archived palette */\n{palette}\n*/\n{live}", 1)
        self.assertNotEqual(gen.low_chat_contrast(gen.load(), source), [])


if __name__ == "__main__":
    unittest.main()
