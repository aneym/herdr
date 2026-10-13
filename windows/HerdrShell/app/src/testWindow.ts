// The PC test window (pc.py run --test-window) runs beside Alex's own Shell on the same
// server. It may observe any terminal but never takes one from another client: on 2026-10-13
// a test window opening the CoS tab took its terminal from Alex's PC Shell, which then showed
// the pane closed. main.rs sets this flag in the test window only.
export function isTestWindow() {
  return (globalThis as { __HERDR_TEST_WINDOW__?: boolean }).__HERDR_TEST_WINDOW__ === true;
}
