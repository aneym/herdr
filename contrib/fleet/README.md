# herdr-fleet

`herdr-fleet` drives panes and agents on every herdr machine from one command line. It is a single Python 3 script with no dependencies. Every call shells out to `herdr` or to `herdr --machine <label>`, so it needs no server changes and works against any 0.9 server that answers the machine API.

Pane ids belong to one server: `w1:p1` exists on every machine. The fleet always names a pane as `<machine>/<pane>`, for example `studio/w3:p1` or `book/w1:p2`. The caller's own server is `local`, or whatever `HERDR_FLEET_LOCAL` says. Every other name is a saved machine from `herdr machine list`.

## Commands

| Command | What it does |
|---|---|
| `hosts [--json]` | Probe each machine: reachable, herdr version, protocol, agent counts by status. Exits 1 if any machine is down. |
| `ls [--machine M] [--agents] [--status S]... [--json]` | List panes on all machines in parallel. Agent panes carry their status and name; plain shells show `-` and `shell`. `--agents`, or any `--status`, lists agent panes only. A machine that fails shows up as `{machine, error}` and the exit code is 1. |
| `get <m>/<pane>` | Print the agent in that pane as JSON, or the pane itself when no agent runs there. |
| `read <m>/<pane> [--lines N] [--source S]` | Print the pane's terminal text (`herdr pane read`). |
| `run <m>/<pane> <command> [--timeout MS] [--no-wait]` | Type a shell command into the pane's shell, wait for it to finish, print its output, and exit with its status. The default timeout is 60000 ms; on timeout it exits 124 and the command keeps running. `--no-wait` only types the command. |
| `keys <m>/<pane> <key>...` | Press keys in a pane: `enter`, `esc`, `down`, `ctrl+c`, and so on. Use it to answer a dialog such as Claude Code's folder-trust prompt. |
| `prompt <m>/<pane> <text> [--wait] [--until S]... [--timeout MS]` | Submit a prompt to an agent (`herdr agent prompt`). |
| `wait <m>/<pane> [--until S]... [--timeout MS]` | Wait for an agent state (`herdr agent wait`). |
| `spawn <m> --kind K [--cwd DIR] [--label L] [--prompt TEXT] [--timeout MS] [-- agent-args...]` | Open a new workspace on machine `m` and start an agent of kind `K` in it. On failure the error names the pane, which stays open so you can read it. |
| `attention [--status S]... [--json] [--file-unblock]` | List agents in the given states across machines; the default state is `blocked`. With `--file-unblock` it files one unblock ask for each newly waiting agent. |
| `whoami` | Print the calling pane as `<machine>/<pane>`. |

`get`, `read`, `prompt` and `wait` print herdr's own JSON or text unchanged.

### How `run` finds the output

`run` wraps the command as
`printf '__fleet_%s_%s\n' begin <nonce>; eval '<command>'; printf '__fleet_%s_%s_%s\n' end <nonce> "$?"`.
It waits for the end marker with `herdr pane wait-output`, then reads the pane unwrapped and prints the lines between the two markers. `printf` assembles the markers at run time, so the echoed command line never matches them. `eval` keeps `#` comments and `&` inside the command. The pane must be at a POSIX-style shell prompt (sh, bash or zsh; fish is not supported). Output longer than the pane's scrollback loses its start, and `run` warns when that happens.

## Examples

```sh
herdr-fleet hosts
herdr-fleet ls --agents --status blocked
herdr-fleet run book/w1:p1 'git -C ~/repos/app status --short'
herdr-fleet spawn book --kind claude --cwd ~/repos/app --label fix-login
herdr-fleet prompt book/w4:p1 "run the tests and report" --wait --until done --timeout 600000
herdr-fleet read book/w4:p1 --lines 40
```

## Environment

| Variable | Default | Meaning |
|---|---|---|
| `HERDR_BIN` | `herdr` on `PATH` | The herdr binary to call. |
| `HERDR_FLEET_LOCAL` | `local` | The name for the caller's own server in ids. |
| `HERDR_FLEET_TIMEOUT` | `30` | Per-call timeout in seconds for each herdr request. |
| `UNBLOCK_BIN` | `unblock` on `PATH` | The unblock CLI that `attention --file-unblock` calls. |
| `HERDR_FLEET_STATE_DIR` | `$XDG_STATE_HOME/herdr-fleet`, else `~/.local/state/herdr-fleet` | Where `--file-unblock` records what it filed (`filed.json`). |

## Safety

- `attention --file-unblock` files real asks in Alex's unblock queue. Keep it off in tests. To test it, point `UNBLOCK_BIN` at a stub that saves its stdin and prints `{"ticket": "fake-1"}`, and point `HERDR_FLEET_STATE_DIR` at a scratch directory. Do not use `XDG_STATE_HOME` for that: herdr keeps its saved-machine list under it, so the fleet would stop seeing remote machines.
- Each ask is a v2 `question` with one text field. It carries the pane's last lines and `origin.agent = "herdr-fleet"`, with no pane id, so unblock never types the answer into the caller's own pane. Nobody types the answer into the waiting pane either; reply with `herdr-fleet prompt`. The same agent state is filed once. The key is pane, agent id and `state_change_seq`.
- `run`, `keys` and `prompt` type into live terminals. Check the target with `get` first.
- Never point a test at a live herdr server. Start a lab server with its own `XDG_CONFIG_HOME` and `XDG_STATE_HOME`, and unset `HERDR_SOCKET_PATH`, `HERDR_CLIENT_SOCKET_PATH`, `HERDR_SESSION`, `HERDR_ENV`, `HERDR_PANE_ID`, `HERDR_TAB_ID` and `HERDR_WORKSPACE_ID`. Inside a herdr pane those variables point at the live server. `herdr status server` prints the socket it will use.
- `herdr machine add` runs on the remote the first `herdr` it finds: `command -v herdr`, then `$HOME/.local/bin/herdr`, `/opt/homebrew/bin/herdr`, `/usr/local/bin/herdr` and a few mise and nix paths. If none of them is compatible, it offers to install over `~/.local/bin/herdr` on the remote. The remote socket is `$XDG_CONFIG_HOME/herdr/herdr.sock`, or `~/.config/herdr/herdr.sock`. Setting `HERDR_REMOTE_BINARY` skips the search and, after a confirm prompt, always installs that file to the remote's `~/.local/bin/herdr`. For a remote lab, pin `HOME`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME` and `PATH` on the remote side before `machine add`.
- `herdr machine remove` takes the profile id from `machine list --json`, not the label.
- Right after a Claude Code folder-trust prompt is answered, the first `prompt` can come back `agent_prompt_stalled` with the text lost. Send it again.
