#!/usr/bin/env bash
set -euo pipefail
[[ $# == 2 ]] || { echo "usage: $0 <baseline-bin> <candidate-bin>" >&2; exit 2; }
baseline=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
candidate=$(cd "$(dirname "$2")" && pwd)/$(basename "$2")
panes=${HERDR_ATTACH_PANES:-12}; streaming=${HERDR_ATTACH_STREAMING:-2}; rate=${HERDR_ATTACH_RATE:-60}
seconds=${HERDR_PERF_SAMPLE_SECONDS:-30}; warmup=${HERDR_PERF_WARMUP_SECONDS:-3}; rounds=${HERDR_ATTACH_ROUNDS:-3}
out=${HERDR_ATTACH_OUT:-/var/tmp/herdr-attach-perf-$(date +%Y%m%dT%H%M%S)-$$.json}
for value in "$panes" "$streaming" "$seconds" "$rounds"; do
  [[ $value =~ ^[1-9][0-9]*$ ]] || { echo "panes, streaming, seconds and rounds must be positive integers" >&2; exit 2; }
done
[[ $warmup =~ ^[0-9]+$ && $rate =~ ^[0-9]+([.][0-9]+)?$ ]] && awk -v r="$rate" 'BEGIN {exit !(r>0)}' && ((streaming <= panes)) || { echo "invalid warmup, rate or streaming count" >&2; exit 2; }
for tool in tmux jq lsof python3 perl; do command -v "$tool" >/dev/null; done
[[ -x $baseline && -x $candidate ]] || { echo "both binaries must be executable" >&2; exit 2; }
producer=$(cd "$(dirname "$0")" && pwd)/release_perf_producer.pl
root=$(mktemp -d /var/tmp/herdr-attach-perf.XXXXXX); name="hap$$"; top_pid=
tmux_cmd=(tmux -L "$name"); clean_env=(env)
while IFS= read -r key; do case "$key" in HERDR_*) clean_env+=(-u "$key");; esac; done < <(compgen -e)
cleanup_case() {
  if [[ -n $top_pid ]]; then if kill -0 "$top_pid" 2>/dev/null; then kill "$top_pid"; fi; if wait "$top_pid"; then :; fi; top_pid=; fi
  if [[ -n ${bin:-} ]]; then
    if "${control[@]}" "$bin" session stop "$name" >/dev/null 2>&1; then :; fi
    for ((n=0;n<50;n++)); do if "${control[@]}" "$bin" session delete "$name" >/dev/null 2>&1; then break; fi; sleep 0.1; done
  fi
  if "${tmux_cmd[@]}" kill-server 2>/dev/null; then :; fi
}
cleanup() { cleanup_case; rm -rf "${root:?}"; }
trap cleanup EXIT; trap 'exit 130' INT; trap 'exit 143' TERM
cpu_time() { ps -o time= -p "$1" | awk -F: '{s=0; for(i=1;i<=NF;i++) s=s*60+$i; printf "%.6f\n",s}'; }
check_attaches() {
  for window in "${windows[@]}"; do
    info=$("${tmux_cmd[@]}" display-message -p -t "$window" '#{pane_dead} #{pane_pid} #{pane_width} #{pane_height}')
    read -r dead pid width height <<< "$info"
    [[ $dead == 0 && $width == 120 && $height == 40 ]] && kill -0 "$pid" 2>/dev/null || { echo "attach client died or has invalid PTY: $window ($info)" >&2; exit 1; }
  done
}
for ((round=1;round<=rounds;round++)); do
  variants='baseline candidate'; if ((round%2==0)); then variants='candidate baseline'; fi
  for scenario in idle stream; do for variant in $variants; do
    bin=$baseline; if [[ $variant == candidate ]]; then bin=$candidate; fi
    state="$root/$round-$scenario-$variant"; mkdir -p "$state/xdg" "$state/run"
    launch=("${clean_env[@]}" XDG_CONFIG_HOME="$state/xdg" XDG_RUNTIME_DIR="$state/run" HERDR_DISABLE_SOUND=1 SHELL=/bin/sh)
    control=("${launch[@]}" HERDR_SESSION="$name")
    printf -v command '%q ' "${launch[@]}" "$bin" --session "$name"
    "${tmux_cmd[@]}" new-session -d -s "$name" -x 86 -y 47 "exec $command"
    for ((n=0;n<150;n++)); do if data=$("${control[@]}" "$bin" pane list 2>/dev/null); then break; fi; sleep 0.1; done
    pane=$(jq -er '.result.panes[0].pane_id' <<< "$data"); ids=("$pane")
    workspace=$("${control[@]}" "$bin" workspace list | jq -er '.result.workspaces[0].workspace_id')
    for ((i=2;i<=panes;i++)); do
      pane=$("${control[@]}" "$bin" tab create --workspace "$workspace" --label "bench-$i" --no-focus | jq -er '.result.root_pane.pane_id'); ids+=("$pane")
    done
    windows=(); i=0
    for pane in "${ids[@]}"; do
      terminal=$("${control[@]}" "$bin" pane get "$pane" | jq -er '.result.pane.terminal_id')
      printf -v command '%q ' "${control[@]}" "$bin" terminal attach "$terminal" --no-escape
      window=$("${tmux_cmd[@]}" new-window -d -P -F '#{pane_id}' -t "$name" "exec $command")
      "${tmux_cmd[@]}" set-option -w -t "$window" remain-on-exit on
      "${tmux_cmd[@]}" resize-window -t "$window" -x 120 -y 40
      windows+=("$window"); i=$((i+1))
      if [[ $scenario == stream && $i -le $streaming ]]; then "${control[@]}" "$bin" pane run "$pane" "$producer" "$rate" "$state/gate" "p$i" >/dev/null; fi
    done
    touch "$state/gate"
    socket="$state/xdg/herdr/sessions/$name/herdr.sock"; server_pid=
    for ((n=0;n<80;n++)); do
      if pids=$(lsof -t "$socket" 2>/dev/null); then server_pid=$(head -n1 <<< "$pids"); break; fi; sleep 0.1
    done
    [[ -n $server_pid ]] || { echo "could not find isolated server pid" >&2; exit 1; }
    sleep "$warmup"; check_attaches
    before=$(cpu_time "$server_pid"); [[ -n $before ]]
    if [[ $(uname -s) == Darwin ]] && command -v top >/dev/null; then LC_ALL=C top -l 2 -s "$seconds" -pid "$server_pid" -stats pid,idlew > "$state/top" 2>/dev/null & top_pid=$!; fi
    sleep "$seconds"
    after=$(cpu_time "$server_pid"); [[ -n $after ]]; check_attaches
    if [[ -n $top_pid ]]; then if wait "$top_pid"; then :; fi; top_pid=; fi
    idlew=null
    if [[ -f $state/top ]]; then idlew=$(awk -v p="$server_pid" '$1==p {n++; if(n==2 && $2 ~ /^[0-9]+$/) v=$2} END {print (v=="" ? "null" : v)}' "$state/top"); fi
    jq -nc --arg variant "$variant" --arg scenario "$scenario" --argjson round "$round" --argjson before "$before" --argjson after "$after" --argjson seconds "$seconds" --argjson idlew "$idlew" '{variant:$variant,scenario:$scenario,round:$round,cpu_seconds:($after-$before),cpu_percent:(($after-$before)/$seconds*100),idle_wakeups:$idlew}' >> "$root/raw.jsonl"
    cleanup_case; bin=
  done; done
done
python3 - "$root/raw.jsonl" "$out" "$panes" "$streaming" "$rate" "$seconds" "$warmup" "$rounds" <<'PY'
import json, statistics, sys
rows = [json.loads(line) for line in open(sys.argv[1])]
summary = []
print('scenario   baseline median CPU%  candidate median CPU%    change %')
for scenario in ('idle', 'stream'):
    b, c = [statistics.median(r['cpu_percent'] for r in rows if r['scenario'] == scenario and r['variant'] == v) for v in ('baseline', 'candidate')]
    change = (c-b)/b*100 if b else None
    summary.append(dict(scenario=scenario, baseline_median_cpu_percent=b, candidate_median_cpu_percent=c, change_percent=change))
    print(f'{scenario:10} {b:20.3f} {c:22.3f} {format(change, "+.1f") if change is not None else "n/a":>11}')
print('round scenario variant       CPU seconds      CPU% idle wakeups')
for r in rows:
    print(f'{r["round"]:5} {r["scenario"]:8} {r["variant"]:10} {r["cpu_seconds"]:12.3f} {r["cpu_percent"]:9.3f} {str(r["idle_wakeups"]):>12}')
settings = dict(zip(('panes', 'streaming', 'rate', 'sample_seconds', 'warmup_seconds', 'rounds'), map(float, sys.argv[3:])))
with open(sys.argv[2], 'w') as f:
    json.dump(dict(settings=settings, summary=summary, rounds=rows), f, indent=2)
print('JSON: ' + sys.argv[2])
if any(r['cpu_seconds'] <= 0 for r in rows if r['scenario'] == 'stream'):
    sys.exit('error: stream case measured zero CPU')
PY
