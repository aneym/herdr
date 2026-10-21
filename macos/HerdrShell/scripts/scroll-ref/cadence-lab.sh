#!/bin/sh
# cadence-lab.sh HERDR APP: in the Cua Space, a herdr server on the guest itself (the Shell
# talks to a local server on a real Mac; the ssh bridge to a Studio lab adds its own
# jitter), one pane of 3000 numbered lines, and the dev Shell APP attached to it.
# cadence-lab.sh HERDR ghostty: the same lines in Ghostty.app with the Shell's font config.
# cadence-lab.sh HERDR stop: stops both.
H=$1 APP=$2 L=/tmp/herdr-cadence
S=$L/c/herdr/sessions/cad/herdr.sock
pkill -f '/Users/lume/(scroll-cadence|.herdr-space/bin)/HerdrShell'
pkill -x ghostty
[ -S $S ] && HOME=$L XDG_CONFIG_HOME=$L/c HERDR_SOCKET_PATH=$S $H --session cad server stop >/dev/null 2>&1
rm -rf $L
[ "$APP" = stop ] && exit 0
if [ "$APP" = ghostty ]; then
  open -na ~/Applications/Ghostty.app --args --font-family="SF Mono" --font-size=13.5 \
    --adjust-cell-height=8% --window-width=100 --window-height=40 \
    --confirm-close-surface=false \
    -e /bin/sh -c "seq -f 'line %g' 1 3000; exec /bin/cat"
  sleep 3
  echo ready
  exit 0
fi
mkdir -p $L/c/herdr
printf '[session]\nresume_agents_on_restore = false\n' > $L/c/herdr/config.toml
echo '[]' > $L/c/herdr/plugins.json
printf "PS1='%%1~ %%# '\n" > $L/.zshrc; : > $L/.zshenv
(
  export HOME=$L XDG_CONFIG_HOME=$L/c XDG_STATE_HOME=$L/s XDG_CACHE_HOME=$L/k XDG_DATA_HOME=$L/d \
    SHELL=/bin/zsh TERM=xterm-256color HERDR_SOCKET_PATH=$S
  nohup $H --session cad server > $L/server.log 2>&1 < /dev/null &
  for i in $(seq 1 100); do $H --session cad workspace list >/dev/null 2>&1 && break; sleep 0.1; done
  P=$($H --session cad workspace create --label cadence --cwd /tmp |
      /usr/bin/python3 -c 'import json,sys;print(json.load(sys.stdin)["result"]["root_pane"]["pane_id"])')
  sleep 1
  $H --session cad pane run $P "clear; seq -f 'line %g' 1 3000" >/dev/null
)
sleep 1.5
cd /Users/lume
nohup env HERDR_SOCKET_PATH=$S $APP --herdr $H --socket $S \
  --ghostty-resources /Users/lume/.herdr-space/ghostty --keymap /Users/lume/scroll/keymap.json > $L/app.log 2>&1 < /dev/null &
sleep 6
echo ready
