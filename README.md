# herdr


<p align="center">
  <img src="assets/logo.png" alt="herdr" width="100" />
</p>

<p align="center">
  <a href="https://herdr.dev">herdr.dev</a> · <a href="#install">install</a> · <a href="https://herdr.dev/docs/quick-start/">quick start</a> · <a href="https://herdr.dev/docs/">docs</a>
</p>

<p align="center">
  English · <a href="README.zh-CN.md">简体中文</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-666666?labelColor=333333" alt="Apache 2.0 license" /></a>
  <a href="https://github.com/herdrdev/herdr/releases"><img src="https://img.shields.io/github/downloads/herdrdev/herdr/total?labelColor=333333&color=666666" alt="total GitHub release downloads" /></a>
  <a href="https://github.com/herdrdev/herdr/stargazers"><img src="https://img.shields.io/github/stars/herdrdev/herdr?labelColor=333333&color=666666&logo=github" alt="GitHub stars" /></a>
  <a href="https://github.com/herdrdev/herdr/releases/latest"><img src="https://img.shields.io/github/v/release/herdrdev/herdr?label=release&labelColor=333333&color=666666" alt="latest stable release" /></a>
  <a href="https://formulae.brew.sh/formula/herdr"><img src="https://img.shields.io/homebrew/v/herdr?label=homebrew&labelColor=333333&color=666666" alt="Homebrew version" /></a>
  <a href="https://x.com/herdrdev"><img src="https://img.shields.io/badge/follow-%40herdrdev-000000?logo=x&logoColor=white" alt="follow @herdrdev on X" /></a>
</p>

---

https://github.com/user-attachments/assets/043ec09f-4bdd-41d5-aee0-8fda6b83e267

**the runtime your coding agents live on.**

- **always running** — herdr is a background server; the terminals live inside it. close the lid, drop the network, or restart the machine; agents keep working and sessions come back. reattach from any terminal, or over ssh.
- **never hunt for the stuck one** — every pane is marked working, blocked, or idle. when an agent stops and needs an answer, herdr says so.
- **agent-native** — agents drive herdr through the cli and socket api: they can spawn panes, prompt each other, and wait until another agent is genuinely blocked. [agent skill →](https://herdr.dev/docs/agent-skill/)
- **runs what you already run** — claude code, codex, cursor, opencode, grok and the rest. herdr doesn't wrap or replace them; it owns their terminals.
- **keyboard and mouse, both first-class** — tmux-style prefix keys *and* click, drag, split. pick per moment, not per tool.
- **plugins** — extend panes and workflows. [browse the marketplace →](https://herdr.dev/plugins/)
- **one rust binary, no electron** — runs in whatever terminal you already use.

---

## this fork: deep Ghostty integration

This fork ([aneym/herdr](https://github.com/aneym/herdr)) tracks upstream and adds a set of
features built for one goal: herdr living inside [Ghostty](https://ghostty.org) as a native-feeling
macOS app, with every control on ⌘ instead of ctrl. The fork features (tab status icons, sidebar
section order and row sizing, deferred attention-read, per-mode theme overrides, configurable state
icons, triage sort, ⌘K search palette, focus history, workspace profiles, and the ⌘C copy bridge)
exist to serve that setup. The integration itself needs no code changes in Ghostty — it is
configuration layering plus this fork's keybinding surface. Here is the whole recipe.

### the layer model

Every chord a human presses lives on ⌘ or alt. ctrl appears nowhere except as invisible wire
encoding inside config files:

- **alt** — window manager (AeroSpace) owns it.
- **⌘** — herdr in-app navigation, but *only inside the herdr window*.
- **alt+p** — the herdr prefix (`[keys] prefix = "alt+p"`), replacing the unreachable ctrl+b.

### a dedicated Ghostty instance, so normal terminals keep native ⌘

Ghostty has no per-window keybinds, so overriding ⌘W or ⌘K globally would break every normal
terminal. Instead, herdr runs in its own Ghostty instance launched with an extra config layer:

```
# ~/.config/ghostty/herdr.conf — loaded only by the herdr instance
title = herdr
command = herdr          # or: ssh -t <host> herdr for a remote server
macos-icon = custom
macos-custom-icon = ~/.config/ghostty/herdr.icns
auto-update = off
```

The trick that makes it a real app: clone Ghostty.app into `HerdrTerm.app` with its **own
CFBundleIdentifier**. macOS treats two instances of one bundle id as one app — Spotlight, the Dock,
and ⌘Tab will happily focus the wrong one. A distinct bundle id gives herdr its own icon, its own
⌘Tab entry, and its own window-manager rules. Two details matter:

- Pin `XDG_CONFIG_HOME` in the clone's `LSEnvironment` to a chain file that includes the normal
  Ghostty config, the Application Support config, **and** herdr.conf. Ghostty derives its
  Application Support config path from the bundle id, so the clone silently skips your real one
  unless you chain it explicitly (fonts, themes, and shell integration vanish otherwise — and
  herdr's theme auto-detect defaults to dark when the OSC 10/11 color query goes unanswered).
- Set `LSMultipleInstancesProhibited` in the clone's Info.plist *and* launch with `open -a` (never
  `open -na` — `-n` always spawns a duplicate instance, bypassing the plist key).

Delete the Sparkle `SUFeedURL` from the clone so auto-update can never overwrite its identity, and
rebuild the clone after each Ghostty update.

### ⌘ chords become byte relays

Ghostty keybinds in herdr.conf translate each ⌘ chord into the CSI-u byte sequence of a ctrl+alt
chord; herdr binds those bytes. The ctrl+alt names in config are pure wire encoding — nobody types
them:

```
# herdr.conf (Ghostty side): ⌘ chord → bytes
keybind = super+h=text:\x1b[104;7u        # ⌘H → ctrl+alt+h
keybind = super+k=text:\x1b[102;7u        # ⌘K → search palette
keybind = super+left_bracket=text:\x1b[91;7u
keybind = super+physical:one=text:\x1b[49;3u   # ⌘1 → alt+1
```

```toml
# config.toml (herdr side): bind the bytes
[keys]
prefix = "alt+p"
focus_left = "ctrl+alt+h"          # arrives when ⌘H is pressed
search = ["prefix+f", "ctrl+alt+f"]
focus_back = ["prefix+[", "ctrl+alt+["]
focus_forward = ["prefix+]", "ctrl+alt+]"]
switch_tab = "alt+1..9"
switch_workspace = "ctrl+alt+1..9"
focus_agent = "prefix+1..9"
close_pane = ["prefix+x", "ctrl+alt+x"]
next_agent = "ctrl+alt+u"
previous_agent = "ctrl+alt+y"
```

The resulting map, all herdr-window-only:

| chord | action |
| --- | --- |
| ⌘H/J/K/L | focus pane left/down/up/right (⌘K goes to the palette; ⌘⇧K is focus up) |
| ⌘⌥J / ⌘⌥K | walk the agent list up / down |
| ⌘D / ⌘⇧D | split vertical / horizontal |
| ⌘T | new tab · ⌘P previous tab · ⌘W close pane (⌘⇧W stays native close-window) |
| ⌘E | cycle chats in sidebar priority order · ⌘O jump to the agent needing attention |
| ⌘K | fuzzy search palette over workspaces and chat threads (fork feature) |
| ⌘[ / ⌘] | browser-style pane focus history, back / forward (fork feature) |
| ⌘N | spawn-agent popup (pick agent, workspace, initial message; runs detached) |
| ⌘B | toggle sidebar · ⌘G goto tree |
| ⌘1–9 | switch tab · ⌘⌥1–9 switch workspace · prefix+1–9 focus agent N |

### clipboard, without surprises

- `[ui] copy_on_select = false` — highlighting never auto-copies; the selection stays highlighted.
- `keybind = performable:super+c=copy_to_clipboard:mixed` — if Ghostty owns a selection, ⌘C copies
  it natively; otherwise the chord falls through as bytes and herdr copies *its* retained
  selection. One key, both layers, no dead ⌘C.
- Panes running mouse-reporting apps (Claude Code and friends) get their drags forwarded, so herdr
  can't see that selection at all. This fork tracks forwarded drag gestures and translates ⌘C into
  the ctrl+c those apps actually copy with — and swallows it when there was no drag, so ⌘C never
  interrupts an agent by accident. The fork also exports `TERM_PROGRAM=ghostty` into pane
  environments so kitty-keyboard-protocol apps negotiate the right encoding.
- `keybind = performable:super+v=paste_from_clipboard` — text pastes natively (instant, bracketed);
  an image-only clipboard makes the native paste report "not performed" and the chord falls through
  to herdr's image handling.

### clickable links under mouse capture

herdr is mouse-first, so it captures the mouse — which normally kills Ghostty's link detection.
One line restores it:

```
mouse-shift-capture = never
```

Ghostty's only link path while an app captures the mouse is the shift bypass: **⌘⇧-hover**
underlines and previews, **⌘⇧-click** opens — covering both plain-text URLs (Ghostty's regex) and
OSC 8 hyperlinks, which herdr passes through to the host terminal. Links open on the machine you
are sitting at, even when the herdr server is remote. Audit any config in your chain for
`mouse-shift-capture = always`; it disables the bypass entirely.

---

## install

```bash
curl -fsSL https://herdr.dev/install.sh | sh
```

or `brew install herdr` · `mise use -g herdr` · windows: `powershell -ExecutionPolicy Bypass -c "irm https://herdr.dev/install.ps1 | iex"` · [binaries](https://github.com/herdrdev/herdr/releases)

then start it where the work lives:

```bash
herdr
```

run your agents, split panes, walk away. `ctrl+b q` detaches, `herdr` reattaches. [quick start →](https://herdr.dev/docs/quick-start/)

## docs

everything lives at [herdr.dev/docs](https://herdr.dev/docs/): [quick start](https://herdr.dev/docs/quick-start/) · [concepts](https://herdr.dev/docs/concepts/) · [supported agents](https://herdr.dev/docs/agents/) · [keyboard](https://herdr.dev/docs/keyboard/) · [configuration](https://herdr.dev/docs/configuration/) · [session state](https://herdr.dev/docs/session-state/) · [remote](https://herdr.dev/docs/persistence-remote/) · [integrations](https://herdr.dev/docs/integrations/) · [plugins](https://herdr.dev/docs/plugins/) · [socket api](https://herdr.dev/docs/socket-api/)

## thanks

every past sponsor and backer is listed in [SPONSORS.md](./SPONSORS.md) — thank you 🐑

enterprise / partnership: hey@herdr.dev

## agent instructions

if you are an ai agent helping with this repository, read [`AGENTS.md`](./AGENTS.md) before making changes and read [`CONTRIBUTING.md`](./CONTRIBUTING.md) before opening issues or PRs.

## development

```bash
git clone https://github.com/herdrdev/herdr
cd herdr
cargo build --release

just test        # unit tests
just check       # formatting, tests, and maintenance checks
```

## license

Herdr is licensed under the [Apache License 2.0](LICENSE).
