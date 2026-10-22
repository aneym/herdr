import { useCallback, useEffect, useRef, useState } from "react";
import { bridge } from "./bridge";

export function useThoughts() {
  const [count, setCount] = useState(0);
  const revision = useRef(0);
  const refresh = useCallback(async () => {
    const current = ++revision.current;
    try {
      const reply = await bridge.thoughtsCount();
      if (current === revision.current) setCount(reply.count);
    } catch {}
  }, []);
  useEffect(() => {
    void refresh();
    const timer = window.setInterval(() => void refresh(), 10000);
    return () => { window.clearInterval(timer); revision.current++; };
  }, [refresh]);
  const save = useCallback(async (text: string) => {
    const reply = await bridge.thoughtSave(text);
    revision.current++;
    setCount(reply.count);
    void refresh();
  }, [refresh]);
  return { count, save };
}
