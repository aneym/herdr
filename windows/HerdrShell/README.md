# Herdr Shell (Windows)

Tauri 2 shell scaffold for Herdr on Windows 11. One xterm.js terminal
(WebGL + unicode11, Cascadia Mono 13, Catppuccin Mocha) in local-echo demo
mode — no server connection yet — plus a named-pipe control channel
(`\\.\pipe\herdr-shell-control-<username>`) for test automation:
`ping`, `state`, `shot`, `type`, `read`.

## Layout

- `app/` — Vite + React 18 + TypeScript UI
- `app/src-tauri/` — Tauri 2 crate `herdr-shell`, binary `HerdrShell`;
  `control.rs` has the pipe server and window capture
- `scripts/` — `pc.py` (runs on the Mac) and the PowerShell helpers it
  scp's to `C:\Users\aneym\winshell\scripts`

## Harness

All commands run on Studio and drive the PC over `ssh pc`. Sources are
mirrored to `C:\Users\aneym\winshell\src`; cargo output goes to
`C:\Users\aneym\winshell\target-shell`; installers land in
`C:\Users\aneym\winshell\out`.

```
python3 windows/HerdrShell/scripts/pc.py sync       # mirror the tree
python3 windows/HerdrShell/scripts/pc.py build      # game guard, then NSIS build
python3 windows/HerdrShell/scripts/pc.py install    # silent /S install + verify
python3 windows/HerdrShell/scripts/pc.py status     # game/idle/app JSON
python3 windows/HerdrShell/scripts/pc.py run        # launch on the interactive desktop
python3 windows/HerdrShell/scripts/pc.py ctl '{"cmd":"ping"}'
python3 windows/HerdrShell/scripts/pc.py shot --out shot.png
```

`build` exits 75 while a game process is running and aborts mid-build if
one starts. `run` also requires `idle_s >= 300` (input idle, refreshed via
an interactive scheduled task) unless `--force-idle` is passed.
