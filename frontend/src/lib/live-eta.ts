import { useEffect, useState } from "react";

export function useLiveCountdown(
  etaSeconds: number | undefined,
  resetKey: string,
): number | undefined {
  const [remaining, setRemaining] = useState<number | undefined>(etaSeconds);

  useEffect(() => {
    if (etaSeconds === undefined) {
      setRemaining(undefined);
      return;
    }
    const deadline = Date.now() + Math.max(0, etaSeconds) * 1_000;
    const update = () => {
      setRemaining(Math.max(0, Math.ceil((deadline - Date.now()) / 1_000)));
    };
    update();
    const timer = window.setInterval(update, 1_000);
    return () => window.clearInterval(timer);
  }, [etaSeconds, resetKey]);

  return remaining;
}

export function formatCountdown(seconds: number): string {
  const safe = Math.max(0, Math.round(seconds));
  if (safe < 60) return `${safe}s`;
  const minutes = Math.floor(safe / 60);
  const remainder = safe % 60;
  if (minutes < 60) return `${minutes}m ${String(remainder).padStart(2, "0")}s`;
  const hours = Math.floor(minutes / 60);
  return `${hours}h ${String(minutes % 60).padStart(2, "0")}m`;
}

