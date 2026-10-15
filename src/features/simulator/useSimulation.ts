import { useEffect, useRef, useState } from "react";
import { api, IpcError } from "@/lib/ipc";
import type { ParamError, SimParams, SimResult } from "@/lib/types";

/**
 * Runs the projection in Rust on every parameter change, debounced.
 *
 * The engine lives in Rust rather than being reimplemented in TypeScript for
 * slider responsiveness: two copies of the same financial model is a guaranteed
 * divergence bug. A 120-month run is well under a millisecond, so a short
 * debounce is all the smoothing the UI needs.
 */
export function useSimulation(params: SimParams | null, delayMs = 80) {
  const [result, setResult] = useState<SimResult | null>(null);
  const [errors, setErrors] = useState<ParamError[]>([]);
  const [pending, setPending] = useState(false);
  const seq = useRef(0);

  useEffect(() => {
    if (!params) return;
    const id = ++seq.current;
    setPending(true);
    const t = setTimeout(() => {
      api
        .simRun(params)
        .then((r) => {
          if (seq.current !== id) return; // a newer run superseded this one
          setResult(r);
          setErrors([]);
        })
        .catch((e: unknown) => {
          if (seq.current !== id) return;
          if (e instanceof IpcError && e.payload.kind === "sim_params") {
            setErrors(e.payload.details ?? []);
          } else {
            setErrors([
              { field: "*", message: e instanceof Error ? e.message : String(e) },
            ]);
          }
        })
        .finally(() => {
          if (seq.current === id) setPending(false);
        });
    }, delayMs);
    return () => clearTimeout(t);
  }, [params, delayMs]);

  return { result, errors, pending };
}
