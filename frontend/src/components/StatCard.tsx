import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { CopyButton } from "@/components/CopyButton";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

type Props = {
  src: string;
  alt: string;
  caption?: string;
  delay?: number;
  embedLink?: string;
};

type Phase = "gathering" | "ready" | "error";

const MAX_RETRIES = 6;
const BASE_DELAY_MS = 2_000;
const MAX_DELAY_MS = 30_000;

function retryDelay(attempt: number): number {
  return Math.min(BASE_DELAY_MS * 2 ** attempt, MAX_DELAY_MS);
}

export function StatCard({ src, alt, caption, embedLink }: Props) {
  const [attempt, setAttempt] = useState(0);
  const [phase, setPhase] = useState<Phase>("gathering");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reduceMotion = useReducedMotion();

  const liveSrc = appendParam(src, "animate", "1");
  const sep = liveSrc.includes("?") ? "&" : "?";
  const bust = attempt === 0 ? "" : `${sep}_=${attempt}`;
  const lightSrc = `${liveSrc}${sep}theme=light${bust}`;
  const darkSrc = `${liveSrc}${sep}theme=dark${bust}`;

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, []);

  function handleLoad() {
    if (timerRef.current) {
      clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setPhase("ready");
  }

  function handleError() {
    if (attempt >= MAX_RETRIES) {
      setPhase("error");
      return;
    }
    setPhase("gathering");
    timerRef.current = setTimeout(() => {
      setAttempt((a) => a + 1);
    }, retryDelay(attempt));
  }

  const page = embedLink
    ? embedLink + (embedLink.includes("?") ? "&" : "?") + "ref=readme"
    : "";
  const staticSrc = appendParam(src, "animate", "0");
  const staticSep = staticSrc.includes("?") ? "&" : "?";
  const staticLightSrc = `${staticSrc}${staticSep}theme=light`;
  const staticDarkSrc = `${staticSrc}${staticSep}theme=dark`;
  const embedSnippet = `<a href="${page}">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="${staticDarkSrc}" />
    <img alt="${alt}" src="${staticLightSrc}" />
  </picture>
</a>`;

  return (
    <figure className="card-panel overflow-hidden">
      {caption && (
        <figcaption className="flex items-center justify-between gap-3 border-b border-border bg-muted/40 px-5 py-3">
          <span className="inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground uppercase">
            <span className="size-1.5 shrink-0 rounded-full bg-signal" aria-hidden="true" />
            {caption}
          </span>
          {embedLink && (
            <CopyButton
              value={embedSnippet}
              ariaLabel={`Copy README embed for ${caption}`}
              className="inline-flex min-h-11 items-center gap-1.5 rounded-md border border-border bg-background px-3 py-2 font-mono text-base text-muted-foreground hover:bg-accent hover:text-accent-foreground sm:min-h-0 sm:px-2.5 sm:py-1 sm:text-xs"
              idleLabel="Embed"
            />
          )}
        </figcaption>
      )}
      <div className="relative">
        <motion.div
          initial={false}
          animate={{
            opacity: phase === "ready" ? 1 : 0,
            y: phase === "ready" || reduceMotion ? 0 : 4,
          }}
          transition={{
            duration: reduceMotion
              ? REDUCED_MOTION_DURATION
              : DURATION.enter,
            ease: EASE_OUT,
          }}
          aria-hidden={phase !== "ready"}
        >
          <picture>
            <source media="(prefers-color-scheme: dark)" srcSet={darkSrc} />
            <img
              key={attempt}
              src={lightSrc}
              alt={alt}
              loading="lazy"
              decoding="async"
              onLoad={handleLoad}
              onError={handleError}
              className="block w-full"
            />
          </picture>
        </motion.div>

        <AnimatePresence initial={false}>
          {phase !== "ready" && (
            <motion.div
              key={phase}
              initial={{
                opacity: 0,
                y: reduceMotion ? 0 : 4,
              }}
              animate={{ opacity: 1, y: 0 }}
              exit={{
                opacity: 0,
                transition: { duration: 0.1, ease: EASE_OUT },
              }}
              transition={{
                duration: reduceMotion
                  ? REDUCED_MOTION_DURATION
                  : DURATION.enter,
                ease: EASE_OUT,
              }}
              className="absolute inset-0 flex flex-col items-center justify-center gap-3 px-6 py-12"
              aria-live="polite"
            >
              <div
                className={`h-32 w-full rounded-md bg-muted/50 ${
                  phase === "gathering" ? "motion-safe:animate-pulse" : ""
                }`}
                aria-hidden="true"
              />
              <p className="text-center font-mono text-base tracking-wide text-muted-foreground sm:text-xs">
                {phase === "gathering"
                  ? "Gathering repo-debt data…"
                  : "Still gathering — check back soon."}
              </p>
            </motion.div>
          )}
        </AnimatePresence>
      </div>
    </figure>
  );
}

function appendParam(url: string, key: string, value: string): string {
  const [base, query = ""] = url.split("?", 2);
  const search = new URLSearchParams(query);
  search.set(key, value);
  return `${base}?${search.toString()}`;
}
