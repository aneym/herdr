# HerdrShell (ported from spike A, P2)

Moved from `herdr-shell-spikes/swift-ghosttykit`. Check: `SHELL_LAB=shellspike-p2 python3 scripts/scenario.py --out checks/P2.txt`. `SHELL_LAB` names the lab session and dir (`shellspike[-suffix]`); `HERDR_SHELL_BIN` overrides the lab's herdr binary (default: the spike `--no-escape` build until P1 ships). `scripts/vendor-ghostty.sh` checks `GHOSTTY_SRC` is at the pinned commit b1d2b7e.

A minimal native macOS shell for herdr. The window is AppKit. On the left, a SwiftUI sidebar reads live herdr state and shows three sections: ORCHESTRATOR, LANES and WORKFLOWS, with folded workflow groups, status dots and host badges. It follows `~/.claude/pretty-docs/factory-devenv-mock-2026-09-28.html`. On the right, each herdr pane is a real libghostty surface (GhosttyKit, Metal renderer) whose child process is `herdr terminal attach <terminal_id> --no-escape`.

It is a spike. Only the lab session `shellspike` has run it. The app refuses a live herdr socket unless `--allow-live` is passed.

## Build

```sh
# 1. GhosttyKit static lib (once). Built from ../vendor/ghostty at b1d2b7e (1.3.2-dev) with zig 0.16:
#    cd ../vendor/ghostty && nice -n 10 zig build -Demit-xcframework=true -Dxcframework-target=native
scripts/vendor-ghostty.sh            # copies libghostty-internal.a (126 MB) + ghostty.h

# 2. The app
nice -n 10 swift build -c release    # .build/release/HerdrShell

# 3. herdr with --no-escape (fork branch spike/pane-attach, see below)
cd /Volumes/StudioExt/repos/herdr-worktrees/pane-attach
env PATH="/usr/bin:$PATH" CC=/usr/bin/cc RUSTFLAGS="-C linker=/usr/bin/cc" \
  ZIG=$HOME/.cache/herdr-build/zig-aarch64-macos-0.16.0/zig \
  ZIG_GLOBAL_CACHE_DIR=$HOME/.cache/herdr-build/zig016-cache \
  CARGO_TARGET_DIR=$HOME/.cache/herdr-build/target-pane-attach \
  nice -n 10 cargo build --release
```

## Updates (one click on every machine)

`scripts/install-publish.sh` (run once on Studio, again after `publish.py` changes) sets it up. A push to `origin/main` (`git config herdr-shell.releaseBranch` in the herdr repo) that changes `macos/HerdrShell` fires the repo's reference-transaction hook, which runs `herdr-shell-publish auto`: `release.sh <sha>` builds the prod bundle and stages it on Studio, and the same app is copied into each target's `~/Library/Application Support/HerdrShell/staged` (targets in `~/.config/herdr-shell/targets.json`, Book over ssh). The running app's title bar then shows **Update**; one click swaps `~/Applications/Herdr Shell.app` and relaunches. A machine that was asleep is caught by the 5 min launchd fanout. By hand: `herdr-shell-publish [ref]`, `herdr-shell-publish status`, `herdr-shell-publish install <target>` (a new machine). `herdr-shell-publish data` (launchd, 20 s) copies Studio's lanes/areas/modes files to the targets so Areas and Parked draw there. Log: `~/.cache/herdr-shell-publish/publish.log`.

## Other machines

The Spaces sidebar lists other machines' herdr servers below the local spaces: a header per machine (fold remembered), its workspaces, agent tabs with status, and agentless tabs folded into `shells N`. Selecting a remote tab attaches its panes through that machine's sockets; layout commands (close, zoom, split, rename) go to that machine. A remote tab's Pin/Unpin goes to that machine, and its pinned row carries the same state glyph as its machine row. Chat mode and the Park/Rename context menu stay local-only. herdr's own endpoint catalog is never touched, so the local tree draws exactly as without machines.

Sockets come from `scripts/machine-tunnels/herdr-machine-tunnels` (launchd `com.aneyman.herdr-machine-tunnels`; `scripts/machine-tunnels/install.sh` on Studio, `install.sh --via studio` on Book), which forwards each machine's `herdr.sock` and `herdr-client.sock` to `~/.config/herdr-machines/<name>/`. The prod app reads `~/.config/herdr-machines/tunnels.json`; a dev or lab run reads machines only from `HERDR_SHELL_MACHINES`. Attach needs the server's protocol to match this herdr: a 0.8.2 server shows herdr's version error in the pane. Check: `python3 scripts/check_machines.py --scratch-tab <machine>/<tab> --scratch-pane <pane>` (offscreen host run, types only into the named scratch shell). Remote pins and host footer names against a second lab server, no real machines: `python3 scripts/check_remote_pins.py --host-ok` (offscreen host run, not yet ported to the Cua Space).

## Run (lab only)

```sh
python3 scripts/lab.py up       # lab server 'shellspike', isolated HOME/XDG under ~/.cache/herdr-build/shellspike, seeded tabs
python3 scripts/app.py start    # launches the app against the lab socket (scripts/run.sh, env -i)
python3 scripts/app.py cmd '{"cmd":"state","out":"/tmp/s.json"}'   # test hook (FIFO)
python3 scripts/app.py stop
python3 scripts/lab.py down     # stops ONLY the lab server

python3 scripts/scenario.py     # full check: up, launch, keys, reads, shot, down; writes SCENARIO.txt
```

App flags: `--herdr BIN --socket PATH --control FIFO [--ghostty-config FILE] [--ghostty-resources DIR] [--allow-live]`. At startup the app drops every inherited `HERDR_*` and `CLAUDE*` variable and sets only `HERDR_SOCKET_PATH`.

## Design notes

- **Surfaces** are retained per `terminal_id` in `SurfaceRegistry`, outside the view tree. They are laid out from herdr's own `layouts[].panes[].rect`, so a split looks the way herdr has it.
- **Size before attach.** `ghostty_surface_set_content_scale` and `set_size` run right after `ghostty_surface_new`. Without them, `herdr terminal attach` starts on a 0x0 PTY, exits with `terminal reported a zero-sized grid`, and the pane stays blank for 5 to 17 s until a later resize. With them, first text arrives in 0.3 s.
- **Keys.**
  - ⌘ chords go to the app menu first: ⌘]/⌘[ switch panes, ⌘⇧]/⌘⇧[ switch tabs, ⌘1-9 jump to a tab, ⌘C/⌘V copy and paste. A chord the menu does not claim falls through to Ghostty.
  - Ctrl chords go straight to Ghostty's key encoder.
  - `--no-escape` stops herdr's attach client from reading ctrl+b as its prefix, so ctrl+b reaches the program.
  - The Ghostty config (`Resources/ghostty.conf`) clears every Ghostty keybind except copy and paste.
- **Sidebar** polls `herdr api snapshot` every 1 s. Tabs are classified by these rules:
  - a tab labelled `wf ...`, or a tab whose agent has an owner, is a workflow;
  - a workflow folds under its owner's tab, or goes to WORKFLOWS when it has no owner;
  - the first agent tab in a workspace is ORCHESTRATOR;
  - every other tab is a lane.
  Host comes from the agent's `host` metadata token and defaults to Studio.
- **Look.** Catppuccin Mocha, SF Mono 13.5 and the mock's palette. Alex's own Ghostty config is not loaded, because its ⌘ chords relay to the herdr TUI.

## herdr fork change

Branch `spike/pane-attach` in `/Volumes/StudioExt/repos/herdr-worktrees/pane-attach`, off `hotfix/tree-owned-tabs` (ad18ceeb). It is uncommitted and changes 5 files (+69/-13):

- `herdr terminal attach <id> [--takeover] [--no-escape]` and `herdr agent attach ... --no-escape`. With `--no-escape`, `AttachEscapeState::without_escape()` forwards every byte, ctrl+b and `q` included. Scroll actions are still handled.
- A new unit test, `attach_without_escape_forwards_prefix_and_q`, passes.
- In `cargo test --release --bin herdr attach_`, 51 of 52 passed. The one failure was `terminal_attach_rejects_missing_terminal_and_removes_client` (`bind test listener: Address already in use`). It passed 3 of 3 times when run alone, so it is a parallel-run socket collision and has nothing to do with this diff.

That binary is copied into the lab (`~/.cache/herdr-build/shellspike/bin/herdr`) and used only there.

## Measured (scenario run 2026-09-28T19:36Z, host load 175 on 16 cores)

| What | Number |
|---|---|
| Lab up + seed | 1.13 s |
| Surfaces created after app start | 0.21 s |
| First Ghostty-rendered text after surface creation | 0.32-0.34 s |
| Prompt visible in both panes after launch | 1.24 s |
| Typed line echoed back (herdr pane read) | 0.054 s after the last key |
| ctrl+b / ctrl+c / option+backspace round trip | 0.119 / 0.055 / 0.040 s |
| Sidebar follows a live tab rename | 0.52 s (1 s poll) |
| `herdr api snapshot` poll | 5-76 ms across runs (10.7 ms this run) |
| App memory / CPU | 124 MB RSS, 2.1% CPU |
| Grid, 1400x820 window, two panes | 62x46 each |

## What works

- Real libghostty surfaces, one per herdr pane, attached with option A plus option C (`--no-escape`). Every scenario keystroke reached the pane, confirmed read-only with `herdr pane read`.
- The ⌘] app chord switches panes and does not leak into the terminal. ctrl+b, ctrl+c and option+backspace reach the program.
- The sidebar builds from live state: the orchestrator row has its folded `wf embed wave-a @PC blocked`, and the recruiter lane has its folded `wf recruiter-2320 @PC working`. It also has a detail panel (panes, focused pane) and a hosts row with poll time.
- Selecting a tab rebuilds the pane area from herdr's layout. When the app quits, its attach clients exit.

## What does not work yet

- **Input methods.** There is no IME or marked text (no `NSTextInputClient`), so dead keys and CJK input do not work.
- **Hidden tabs.** Surfaces for hidden tabs stay attached. They keep an attach client each and hold the herdr pane size.
- **Updates** come from a poll, not a subscribe stream. There is no overview (⌥O), no drag-resize of splits, and no mouse reporting past basic press, release and scroll.
- **Screenshots.** `screencapture -l` fails from this shell because it has no Screen Recording grant. `shot.png` comes from the app's own capture instead (a runtime-resolved `CGWindowListCreateImage` on its own window).
- **Test keys** are CGEvents posted to the app's own pid (`CGEvent.postToPid`), never system-wide. The in-process `NSApp.sendEvent` path, used when the window is not key, dropped the first key after a ⌘[ focus switch. Real hardware keys were not tried; that path is unverified and was left out of the scenario.
- The live herdr session has never been attached, and neither has a remote host.

Park, Resume and Approve scope run on the server machine through `herdr-shell-remote` (installed by `scripts/install-publish.sh`). On Book, set `~/.config/herdr-shell/server.json` to `{"ssh":["ssh","studio-ts"],"remote_bin":"~/.local/bin/herdr-shell-remote"}`; without `ssh`, the helper runs locally. `HERDR_SHELL_SERVER_CONFIG` and `HERDR_SHELL_REMOTE_BIN` override those paths for fixtures. Approval always asks for your words before running; returned modes update the sidebar immediately.

## Checks run in the Space

Build on the host, then run checks on the `herdr-qa` Cua Space desktop, never the host desktop:

```sh
nice -n 10 swift build -c release
HERDR_SHELL_SPACE=1 python3 scripts/check_p26.py
python3 -m py_compile scripts/*.py
python3 scripts/space.py stop
```

`scenario.py` bridges only the isolated lab socket, pushes fixture paths, drives the guest
control FIFO, and pulls state and in-app screenshots back into `checks/`. P26 installs
an official Node 22 runtime once in the guest and runs the real herdr-lane tree there.
`app.py start` refuses host launches unless `--host-ok` is explicitly supplied.

The Space runs one app at a time, so `space.py start` (installed as `herdr-shell-space`)
takes the lock `~/.agent-rails/locks/herdr-qa-space/` first, the same lock seats take by
hand (owner file `<name> <epoch>`). While another owner holds it, start waits up to
`--wait` seconds (default 240) and exits 75; `stop` and `down` act only for the holder,
and `stop --if-mine`, which checks use, does nothing for anyone else. `--force` breaks a
lock older than 15 minutes whose owner process is gone. The owner is `HERDR_SPACE_OWNER`,
else the agent session id; `status` shows the holder and its age. Check:
`python3 scripts/check_space_lock.py` (temp HOME, no Space needed).
