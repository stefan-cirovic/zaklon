import { useCallback, useEffect, useRef } from "react";

/** After failures the wait doubles, up to this. */
const MAX_BACKOFF = 60000;

function hidden(): boolean {
  return typeof document !== "undefined" && document.hidden;
}

/**
 * Run `fn` now and then every `ms` while the app is on screen. One run at a
 * time. Nothing runs while the app is in the background (a phone in a
 * pocket keeps its radio idle); coming back runs it at once. When `fn`
 * throws or returns `false`, the wait doubles (up to a minute) until it
 * works again. `ms === null` stops the polling. Returns a function that
 * runs it right away (for example when the hub is back in reach).
 */
export function useVisiblePoll(fn: () => unknown, ms: number | null): () => void {
  const latest = useRef(fn);
  const now = useRef<() => void>(() => {});
  useEffect(() => {
    latest.current = fn;
  });
  useEffect(() => {
    if (ms === null) {
      now.current = () => {};
      return;
    }
    let alive = true;
    let running = false;
    let failures = 0;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const schedule = () => {
      if (!alive || hidden()) return; // picked up again by the visibility change
      if (timer) clearTimeout(timer);
      const wait = failures ? Math.min(ms * 2 ** failures, Math.max(ms, MAX_BACKOFF)) : ms;
      timer = setTimeout(tick, wait);
    };
    const tick = async () => {
      timer = undefined;
      if (!alive || running || hidden()) return;
      running = true;
      try {
        const r = await latest.current();
        failures = r === false ? Math.min(failures + 1, 6) : 0;
      } catch {
        failures = Math.min(failures + 1, 6);
      }
      running = false;
      schedule();
    };
    const onVisibility = () => {
      if (hidden()) {
        if (timer) clearTimeout(timer);
        timer = undefined;
      } else if (!running) {
        if (timer) clearTimeout(timer);
        tick();
      }
    };
    now.current = () => {
      if (running) return;
      if (timer) clearTimeout(timer);
      failures = 0;
      tick();
    };
    document.addEventListener("visibilitychange", onVisibility);
    tick();
    return () => {
      alive = false;
      if (timer) clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [ms]);
  return useCallback(() => now.current(), []);
}

/** Resolves when the app is on screen (at once if it is). */
export function whenVisible(): Promise<void> {
  if (!hidden()) return Promise.resolve();
  return new Promise((resolve) => {
    const on = () => {
      if (hidden()) return;
      document.removeEventListener("visibilitychange", on);
      resolve();
    };
    document.addEventListener("visibilitychange", on);
  });
}
