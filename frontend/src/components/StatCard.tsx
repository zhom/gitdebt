import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { useRenderedTheme } from "@/lib/rendered-theme";

type Props = {
  src: string;
  alt: string;
  caption?: string;
  delay?: number;
  apiBase?: string;
  embedLink?: string;
  priority?: boolean;
  liveRepo?: string;
};

type Phase = "gathering" | "ready" | "error";

const MAX_RETRIES = 6;
const BASE_DELAY_MS = 2_000;
const MAX_DELAY_MS = 30_000;

function retryDelay(attempt: number): number {
  return Math.min(BASE_DELAY_MS * 2 ** attempt, MAX_DELAY_MS);
}

export function StatCard({
  src,
  alt,
  caption,
  apiBase,
  embedLink,
  priority = false,
  liveRepo,
}: Props) {
  const [attempt, setAttempt] = useState(0);
  // The image is useful content even before this island hydrates. Starting in
  // `ready` keeps the server-rendered media visible and avoids the cached-image
  // race where `load` fires before React attaches its handler, leaving the
  // gathering veil on screen forever.
  const [phase, setPhase] = useState<Phase>("ready");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reduceMotion = useReducedMotion();
  const theme = useRenderedTheme();

  const liveSrc = appendParam(
    appendParam(appendParam(src, "context", "app"), "animate", "1"),
    "render",
    MEDIA_RENDER_REVISION,
  );
  const sep = liveSrc.includes("?") ? "&" : "?";
  const bust = attempt === 0 ? "" : `${sep}_=${attempt}`;
  const themedSrc = `${liveSrc}${sep}theme=${theme}${bust}`;

  useEffect(() => {
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    };
  }, []);

  useEffect(() => {
    const targetRepo = liveRepo;
    if (!targetRepo) return;
    function refresh(event: Event) {
      if (!targetRepo) return;
      const detail = (event as CustomEvent<{
        repo?: string;
        analysis?: { phase?: string };
      }>).detail;
      if (
        detail?.repo?.toLowerCase() === targetRepo.toLowerCase() &&
        detail.analysis?.phase === "complete"
      ) {
        if (timerRef.current) clearTimeout(timerRef.current);
        setPhase("gathering");
        setAttempt((value) => value + 1);
      }
    }
    window.addEventListener("gitdebt:repo-progress", refresh);
    return () => window.removeEventListener("gitdebt:repo-progress", refresh);
  }, [liveRepo]);

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

  const chartPath = apiBase && src.startsWith(apiBase) ? src.slice(apiBase.length) : src;

  return (
    <figure className="card-panel relative">
      {caption && (
        <figcaption className="flex items-center justify-between gap-3 border-b border-border bg-muted/40 px-5 py-3">
          <div className="inline-flex items-center gap-2 font-mono text-xs tracking-wide text-muted-foreground uppercase">
            <span className="size-1.5 shrink-0 rounded-full bg-signal" aria-hidden="true" />
            {caption}
          </div>
          {embedLink && apiBase && (
            <EmbedSnippet
              apiBase={apiBase}
              chartPath={chartPath}
              linkHref={embedLink}
              label={caption}
              altText={alt}
              variant="menu"
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
          <img
            key={attempt}
            src={themedSrc}
            alt={alt}
            loading={priority ? "eager" : "lazy"}
            fetchPriority={priority ? "high" : "auto"}
            decoding="async"
            onLoad={handleLoad}
            onError={handleError}
            className="block w-full"
          />
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
