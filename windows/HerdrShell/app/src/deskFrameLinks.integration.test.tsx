// @vitest-environment happy-dom
// Integration: run the real native-injected script in a remote desk frame and
// deliver its postMessage at the browser boundary; only Tauri I/O is supplied.
import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import { Window } from "happy-dom";
import DocPanel from "./DocPanel";
import frameScript from "./deskFrameLinks.js?raw";
const native = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: native.invoke, Channel: class {} }));
(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let cleanup = () => {};
afterEach(() => { cleanup(); vi.restoreAllMocks(); native.invoke.mockReset(); document.body.innerHTML = ""; });
it("opens cross-origin desk links externally without navigating or opening a desk item", async () => {
  native.invoke.mockResolvedValue(undefined);
  (window as unknown as Window).happyDOM.settings.disableIframePageLoading = true;
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host); cleanup = () => act(() => root.unmount());
  await act(async () => root.render(<DocPanel machine="pc" tab="t1" items={[{ name: "Page", kind: "web", url: "https://example.com" }]} active="Page" select={() => {}} error="" />));
  const frame = host.querySelector("iframe")!;
  // Happy DOM does not execute WebView2 initialization or cross-origin message
  // delivery. Run the actual script in its frame context and supply that delivery.
  const remote = new Window({ url: "https://example.com" });
  Object.defineProperty(remote, "top", { value: window });
  Object.defineProperty(remote, "parent", { value: window });
  Object.defineProperty(frame, "contentWindow", { value: remote });
  const doc = remote.document;
  doc.body.innerHTML = '<a href="https://example.net/out"><span>Web</span></a><a href="mailto:desk@example.net">Mail</a><a href="file:///private/file">File</a>';
  vi.spyOn(window, "postMessage").mockImplementation(data => {
    window.dispatchEvent(new MessageEvent("message", { data, source: remote as unknown as MessageEventSource }));
  });
  remote.eval(frameScript);
  for (const anchor of Array.from(doc.querySelectorAll("a")).slice(0, 2)) {
    const click = new remote.MouseEvent("click", { ctrlKey: true, shiftKey: true, button: 0, bubbles: true, cancelable: true });
    anchor.firstChild!.dispatchEvent(click);
    expect(click.defaultPrevented).toBe(true);
    expect(native.invoke).toHaveBeenCalledWith("open_url", { url: anchor.href });
  }
  expect(frame.src).toBe("https://example.com/");
  const count = native.invoke.mock.calls.length;
  window.dispatchEvent(new MessageEvent("message", { data: { kind: "herdr-desk-external-link", href: "https://unrelated.example" }, source: window }));
  window.dispatchEvent(new MessageEvent("message", { data: { kind: "herdr-desk-external-link", href: "file:///private/file" }, source: remote as unknown as MessageEventSource }));
  expect(native.invoke.mock.calls).toHaveLength(count);
  expect(native.invoke.mock.calls.filter(([cmd, args]) => cmd === "api_request" && args.method === "desk.open")).toHaveLength(0);
});
