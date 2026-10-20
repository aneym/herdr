# Herdr typing lag handoff (herdr-input-lag, 2026-10-20)

## Findings
- Herdr Shell (pid 80182) main thread is ~98% idle in three 5 s samples (scratchpad herdr-sample-{1,2,3}.txt). Only timers: HerdrModel.reloadSpacesOverlay, ShotRequest.poll, LaneCatalog.reloadIfChanged. Few ms each. Swift shell source is not under /Volumes/StudioExt/repos (shell/ in the herdr repo only has tokens); not located.
- herdr server (pid 81085) averaged ~51% CPU over 1.5 days. Sample: ~1239/3045 ticks on one thread in render_and_stream_with_graphics_limit, 1051 in render_terminal_virtual. 48 `herdr terminal attach` clients (one per shell tab); every full render (any generic dirty: title, sidebar, agent state, pins) re-rendered all 48 attached terminals in full (src/server/headless/render.rs ~697).
- agent.get was 87% of API traffic: 21 failing calls per minute from unblock-voice wake.js (launchd com.alex.unblock-voice-wake) probing dead panes. Lead stopped that job 15:10Z. I had patched the installed wake.js (backup wake.js.bak-2026-10-20, gone.json tombstone, 6h TTL); lead says voice is being removed, so that patch is moot. agent.get itself is cheap (no text snapshot).
- Load averages were 23-88 with ~42% sys CPU during measurement; system load not ruled out as a contributor.

## Branch input-lag (worktree /Volumes/StudioExt/repos/herdr-wt/input-lag), commit e64cabdd on origin/main 4aaf30a6
- src/pane/terminal/core_cell.rs: CoreCell/CoreGuard wrapping the ghostty core Mutex; deref_mut bumps a generation; lock_quiet() does not.
- Readers that only update caches (render, cursor_state, visible_hyperlinks, detection_text, recent_* snapshots, visible_text) use lock_quiet.
- render_generation() plumbed GhosttyPaneTerminal -> PaneTerminal -> PaneRuntime -> TerminalRuntime.
- src/server/render_stream.rs: AttachRenderKey {generation, epoch, size, cursor}; ClientRenderState::TerminalAnsi.attach_key, cleared by reset_baseline and request_repaint; attach_render_unchanged / note_attach_render.
- src/server/headless/render.rs: attach/observe clients skipped when key equals the last committed one and no repaint pending; key noted after commit_sent_frame or skip_identical.

## Measurements (isolated server, 48 attach clients, tab-rename loop as generic dirty)
- old vs new: server CPU per 8 s dirtied 1.24 s -> 0.29 s; render_and_stream ticks 1654 -> 4; render_terminal_virtual ticks 1311 -> 0; dirty calls completed 99 -> 390; idle CPU 0.13 -> 0.14.
- After one pty write: client 0 got identical 168 bytes (hash b73cc2da2600d14c) in both; other 47 clients 0 bytes; pane reads identical.
- Harness: /private/tmp/claude-501/herdr-lag-harness/h.py (usage: python3 h.py old|new). It sets HOME, XDG_CONFIG_HOME and HERDR_HOME to a temp dir; setting only the socket paths leaks to the live config dir (happened once, live session intact).
- Binaries: /private/tmp/claude-501/herdr-lag-bin/herdr-new, herdr-old. Build needs Zig 0.16.0 (system has 0.15.2): /private/tmp/claude-501/zig16dl/x/zig-aarch64-macos-0.16.0/zig via ZIG=.

## Not done
- Not installed into live Herdr (restart is lead's call). Branch not pushed.
- No cargo test / clippy / fmt run. Scroll, selection, theme change, kitty graphics on attach clients not exercised.
- Live keystroke latency before/after unmeasured; Alex reports lag persists, so the cause may be in the Swift shell or the app render path, not only the server. Next: run herdr-lag-sample (open-factory 37edb5449) during real lag; find the HerdrShell source; check the app-client (non-attach) render path and pane.rs detection loops.
- Other server traffic seen: 110 tab.focus, 92 tab.pin_move, 72 tab.set_pinned per ~3.5 h.
