import { describe, it, expect } from "vitest";
import { pillState } from "./UpdatePill";

describe("pillState", () => {
  // Pure visibility projection guards independent update/rollback availability and absent metadata.
  it("shows only actions supported by the status, including rollback without an update", () => {
    const staged = { sha: "abcdef123456", built_at: "2026-10-06T12:00:00Z" };
    const previous = { sha: "fedcba123456" };
    expect(pillState(null)).toEqual({ update: false, rollback: false, tooltip: "" });
    for (const [available, build, prior, update, rollback] of [
      [false, null, null, false, false],
      [false, staged, null, false, false],
      [true, staged, null, true, false],
      [false, staged, previous, false, true],
      [true, staged, previous, true, true],
      [true, null, previous, false, true],
    ] as const) {
      expect(pillState({ current: "1234567", available, staged: build, previous: prior })).toEqual({
        update, rollback, tooltip: build ? "abcdef1 · 2026-10-06T12:00:00Z" : "",
      });
    }
  });
});
