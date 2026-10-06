import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { WebglAddon } from "@xterm/addon-webgl";
import { Unicode11Addon } from "@xterm/addon-unicode11";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "@xterm/xterm/css/xterm.css";
import "./styles.css";

interface AppInfo {
  version: string;
  commit: string;
  built_at: string;
}

const MOCHA = {
  background: "#1e1e2e",
  foreground: "#cdd6f4",
  cursor: "#f5e0dc",
  cursorAccent: "#1e1e2e",
  selectionBackground: "#45475a",
  black: "#45475a",
  red: "#f38ba8",
  green: "#a6e3a1",
  yellow: "#f9e2af",
  blue: "#89b4fa",
  magenta: "#f5c2e7",
  cyan: "#94e2d5",
  white: "#bac2de",
  brightBlack: "#585b70",
  brightRed: "#f38ba8",
  brightGreen: "#a6e3a1",
  brightYellow: "#f9e2af",
  brightBlue: "#89b4fa",
  brightMagenta: "#f5c2e7",
  brightCyan: "#94e2d5",
  brightWhite: "#a6adc8",
};

export default function App() {
  const hostRef = useRef<HTMLDivElement>(null);
  const [info, setInfo] = useState<AppInfo | null>(null);

  useEffect(() => {
    invoke<AppInfo>("app_info")
      .then(setInfo)
      .catch(() => setInfo(null));
  }, []);

  useEffect(() => {
    const host = hostRef.current;
    if (!host) return;

    const term = new Terminal({
      cols: 80,
      rows: 24,
      fontFamily: '"Cascadia Mono", monospace',
      fontSize: 13,
      theme: MOCHA,
      cursorBlink: true,
    });
    term.loadAddon(new Unicode11Addon());
    term.unicode.activeVersion = "11";
    term.open(host);
    try {
      term.loadAddon(new WebglAddon());
    } catch {
      // WebGL unavailable; canvas renderer is fine for the demo slice.
    }

    const fit = () => {
      const core = term as unknown as {
        _core?: {
          _renderService?: {
            dimensions?: { css?: { cell?: { width?: number; height?: number } } };
          };
        };
      };
      const cell = core._core?._renderService?.dimensions?.css?.cell;
      const cw = cell?.width && cell.width > 0 ? cell.width : 7.8;
      const ch = cell?.height && cell.height > 0 ? cell.height : 17;
      const cols = Math.max(2, Math.floor(host.clientWidth / cw));
      const rows = Math.max(1, Math.floor(host.clientHeight / ch));
      if (cols !== term.cols || rows !== term.rows) term.resize(cols, rows);
    };
    const raf = requestAnimationFrame(fit);
    const ro = new ResizeObserver(fit);
    ro.observe(host);

    // Local-echo demo mode: no server connection yet. Keystrokes and
    // control-pipe "type" payloads go through the same path.
    const handleInput = (data: string) => {
      for (const ch of data) {
        if (ch === "\r" || ch === "\n") term.write("\r\n");
        else if (ch === "\x7f") term.write("\b \b");
        else term.write(ch);
      }
    };
    const sub = term.onData(handleInput);

    term.write("Herdr Shell \x1b[90m(local echo demo)\x1b[0m\r\n$ ");

    const unlType = listen<string>("ctl-type", (e) => handleInput(e.payload));
    const unlRead = listen("ctl-read", () => {
      const buf = term.buffer.active;
      const start = buf.viewportY;
      const end = Math.min(start + term.rows, buf.length);
      const lines: string[] = [];
      for (let i = start; i < end; i++) {
        lines.push(buf.getLine(i)?.translateToString(true) ?? "");
      }
      invoke("ctl_read_result", { text: lines.join("\n") }).catch(() => {});
    });

    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
      sub.dispose();
      unlType.then((f) => f());
      unlRead.then((f) => f());
      term.dispose();
    };
  }, []);

  return (
    <div className="layout">
      <aside className="sidebar">
        <div className="brand">Herdr Shell</div>
        {info && (
          <div className="meta">
            <div>v{info.version}</div>
            <div className="sha">{info.commit}</div>
            <div className="built">{info.built_at}</div>
          </div>
        )}
      </aside>
      <main className="terminal-area">
        <div className="term-host" ref={hostRef} />
      </main>
    </div>
  );
}
