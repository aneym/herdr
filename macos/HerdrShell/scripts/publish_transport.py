import json
import shlex
import subprocess

SSH_OPTS = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=8"]


REMOTE_PROBE = r"""
S="$HOME/Library/Application Support/HerdrShell"
i=$(/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$HOME/Applications/Herdr Shell.app/Contents/Info.plist" 2>/dev/null)
s=$(/usr/bin/plutil -extract commit raw -o - "$S/staged.json" 2>/dev/null)
a=$(/usr/libexec/PlistBuddy -c "Print :HerdrShellCommit" "$S/staged/Herdr Shell.app/Contents/Info.plist" 2>/dev/null)
c=$(/usr/bin/mdfind "kMDItemCFBundleIdentifier == 'com.aneyman.herdr-shell'" 2>/dev/null | wc -l | tr -d ' ')
if [ -d "$HOME/Applications/Herdr Shell.app" ]; then e=1; else e=0; i=""; fi
echo "installed_app=$e"; echo "installed=$i"; echo "staged=$s"; echo "staged_app=$a"; echo "spotlight_copies=$c"
"""


PRUNE_STAGED = r"""
LSR=/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister
R="$S/releases.noindex"
keep=$(readlink "$S/staged" 2>/dev/null || true)
for old in "$S"/releases/incoming.* "$R"/incoming.* "$S"/prev-*.app; do
    [ -e "$old" ] || [ -L "$old" ] || continue
    [ "$old" = "$keep" ] && continue
    find "$old" -maxdepth 2 -name '*.app' -prune -exec "$LSR" -u {} \; 2>/dev/null || true
    rm -rf "$old"
done
rmdir "$S/releases" 2>/dev/null || true
[ -d "$keep" ] && { find "$keep" -maxdepth 2 -name '*.app' -prune -exec "$LSR" -u {} \; 2>/dev/null || true; }
"""


REMOTE_DELIVER = (
    r"""
set -e
S="$HOME/Library/Application Support/HerdrShell"
mkdir -p "$S/releases.noindex"
IN=$(mktemp -d "$S/releases.noindex/incoming.XXXXXX")
trap 'rm -rf "$IN"' EXIT
tar -xzf - -C "$IN"
/usr/bin/codesign --verify "$IN/Herdr Shell.app"
# Migrate the old layout once; subsequent deliveries replace a symlink atomically.
if [ -d "$S/staged" ] && [ ! -L "$S/staged" ]; then
    mv "$S/staged" "$IN/legacy"
fi
if [ ! -L "$S/staged.json" ]; then
    rm -f "$S/staged.json"
    ln -s staged/release.json "$S/staged.json"
fi
LINK="$IN.pointer"
ln -s "$IN" "$LINK"
/bin/mv -fh "$LINK" "$S/staged"
trap - EXIT
"""
    + PRUNE_STAGED
    + r"""
echo delivered
"""
)


def on(t, script):
    """argv that runs script on target t: over ssh, or here for a "local" target (Studio)."""
    if t.get("local"):
        return ["/bin/sh", "-c", script]
    return (
        t["ssh"][:1] + SSH_OPTS + t["ssh"][1:] + ["/bin/sh", "-c", shlex.quote(script)]
    )


def probe(t):
    r = subprocess.run(
        on(t, REMOTE_PROBE), check=False, capture_output=True, text=True, timeout=60
    )
    if r.returncode != 0:
        return None
    return dict(l.split("=", 1) for l in r.stdout.splitlines() if "=" in l)


REMOTE_SERVER = r"""
C="$HOME/.config/herdr-shell/server.json"
[ -e "$C" ] && { echo kept; exit 0; }
mkdir -p "$HOME/.config/herdr-shell"
T="$C.tmp.$$"
cat > "$T" || { rm -f "$T"; exit 1; }
# ln never replaces: a server.json made since the check above wins.
if ln "$T" "$C" 2>/dev/null; then echo written; else echo kept; fi
rm -f "$T"
"""


def send_server_config(t, log):
    try:
        r = subprocess.run(
            on(t, REMOTE_SERVER),
            input=json.dumps(t["server"]),
            check=False,
            capture_output=True,
            text=True,
            timeout=60,
        )
    except (OSError, subprocess.TimeoutExpired) as e:
        log(f"{t.get('name', '?')}: FAIL server config: {e}")
        return
    if "written" in r.stdout:
        log(f"{t.get('name', '?')}: wrote server.json")
    elif r.returncode != 0:
        log(
            f"{t.get('name', '?')}: FAIL server config: {(r.stderr or r.stdout).strip()[-200:]}"
        )


REMOTE_INSTALL = (
    r"""
set -e
if pgrep -x HerdrShell >/dev/null; then echo running; exit 3; fi
A="$HOME/Applications"
mkdir -p "$A"
IN="$A/.herdr-shell-incoming"
rm -rf "$IN"; mkdir "$IN"
tar -xzf - -C "$IN"
/usr/bin/codesign --verify "$IN/Herdr Shell.app"
rm -rf "$A/Herdr Shell.app.previous"
[ -d "$A/Herdr Shell.app" ] && mv "$A/Herdr Shell.app" "$A/Herdr Shell.app.previous"
mv "$IN/Herdr Shell.app" "$A/Herdr Shell.app"
rm -rf "$IN" "$A/Herdr Shell.app.previous"
S="$HOME/Library/Application Support/HerdrShell"
# A staged release from the old layout moves under .noindex; the app is not running.
cur=$(readlink "$S/staged" 2>/dev/null || true)
case "$cur" in "$S"/releases/incoming.*)
    mkdir -p "$S/releases.noindex"
    new="$S/releases.noindex/${cur##*/}"
    mv "$cur" "$new"
    ln -s "$new" "$new.pointer"
    /bin/mv -fh "$new.pointer" "$S/staged";;
esac
"""
    + PRUNE_STAGED
    + r"""
"$LSR" -f "$A/Herdr Shell.app" || true
/usr/bin/mdimport "$A/Herdr Shell.app" 2>/dev/null || true
echo installed
"""
)
