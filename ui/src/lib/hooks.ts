import { useCallback, useEffect, useRef, useState } from "react";
import { ApiError } from "./api";

/** Loads data, ignoring responses from superseded requests. */
export function useAsync<T>(fn: () => Promise<T>, deps: unknown[]) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<ApiError | null>(null);
  const [loading, setLoading] = useState(true);
  const seq = useRef(0);

  const run = useCallback(() => {
    const my = ++seq.current;
    setLoading(true);
    fn()
      .then((d) => {
        if (my === seq.current) {
          setData(d);
          setError(null);
        }
      })
      .catch((e) => {
        if (my === seq.current) setError(e instanceof ApiError ? e : new ApiError("internal", String(e)));
      })
      .finally(() => {
        if (my === seq.current) setLoading(false);
      });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  useEffect(() => {
    run();
  }, [run]);

  return { data, error, loading, reload: run };
}

/** Re-runs `cb` at most every `ms` while `value` keeps changing. */
export function useThrottledEffect(cb: () => void, value: unknown, ms: number) {
  const last = useRef(0);
  const timer = useRef<number | null>(null);
  useEffect(() => {
    const now = Date.now();
    const wait = Math.max(0, ms - (now - last.current));
    if (timer.current) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => {
      last.current = Date.now();
      cb();
    }, wait);
    return () => {
      if (timer.current) window.clearTimeout(timer.current);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [value]);
}
