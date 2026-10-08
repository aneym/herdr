import { describe, expect, it } from "vitest";
import { formatDropPaths, paneAtDrop } from "./dropPaths";

// Pure quoting and geometry algorithms: table-driven edge cases cover shell
// punctuation, Unicode, split boundaries and non-terminal surfaces.
describe("Explorer path drops", () => {
  it.each([
    [["C:\\work"], "C:\\work "],
    [["C:\\my folder"], '"C:\\my folder" '],
    [["C:\\工作\\é.txt"], "C:\\工作\\é.txt "],
    [["C:\\one", "D:\\two words"], 'C:\\one "D:\\two words" '],
    [[], ""],
  ])("formats %j without submitting", (paths, expected) => {
    expect(formatDropPaths(paths)).toBe(expected);
  });
  it.each(["&", "^", "|", "<", ">", "(", ")", "%", "!", ";", "$", "`", "'", "\t"])("quotes %s", special => {
    expect(formatDropPaths([`C:\\a${special}b`])).toBe(`"C:\\a${special}b" `);
  });
  const panes = [
    { id: "left", rect: { left: 240, top: 40, right: 640, bottom: 600 } },
    { id: "right", rect: { left: 650, top: 40, right: 1050, bottom: 600 } },
  ];
  it.each([
    [240, 40, "left"], [639, 599, "left"], [650, 200, "right"],
    [640, 200, null], [100, 200, null], [700, 20, null],
    [1050, 200, null], [700, 600, null],
  ])("maps (%s, %s) to %s, ignoring sidebar/desk/gaps", (x, y, expected) => {
    expect(paneAtDrop(panes, { x, y })).toBe(expected);
  });
});
