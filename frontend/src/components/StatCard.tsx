import { useEffect, useRef, useState } from "react";

import { EmbedSnippet } from "@/components/EmbedSnippet";
import { CAPTION, FIELD } from "@/components/style-tokens";
import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";

/**
 * A rendered chart, mounted on the sheet.
 *
 * The frame states what the drawing is (a FIELD label in the title bar) and
 * offers the one action that belongs to it (take the embed). The image itself
 * is the object, so nothing is drawn over it and nothing is drawn under it.
 *
 * The image is in the markup and painted at first paint. It is never faded in,
 * never revealed by a timeline, and never gated on this island hydrating: the
 * retry state exists only to answer a server that has not finished rendering
 * the chart yet, and until that happens the picture on screen is the picture
 * the server sent.
 */

type Props = {
  src: string;
  alt: string;
  caption?: string;
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
  // The image is useful content before this island hydrates. Starting in
  // `ready` keeps the server-rendered media visible and avoids the cached-image
  // race where `load` fires before React attaches its handler, which would
  // leave a veil on screen forever.
  const [phase, setPhase] = useState<Phase>("ready");
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const theme = useRenderedTheme();

  // Every parameter, in this order, is part of the CDN key the byte-parity
  // goldens assert. Nothing here may be reordered, renamed or dropped.
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
    // Edge-triggered: refresh when analysis *becomes* complete, not on every
    // frame that reports it complete. Each bump appends a fresh `_=N`, which is
    // a brand-new CDN key, so a repeating trigger re-rendered every card at the
    // origin for the rest of the star backfill.
    let wasComplete = false;
    function refresh(event: Event) {
      if (!targetRepo) return;
      const detail = (
        event as CustomEvent<{
          repo?: string;
          analysis?: { phase?: string; complete?: boolean };
        }>
      ).detail;
      if (detail?.repo?.toLowerCase() !== targetRepo.toLowerCase()) return;
      const complete =
        detail.analysis?.phase === "complete" ||
        detail.analysis?.complete === true;
      if (complete && !wasComplete) {
        wasComplete = true;
        if (timerRef.current) clearTimeout(timerRef.current);
        setPhase("gathering");
        setAttempt((value) => value + 1);
      } else if (!complete) {
        wasComplete = false;
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

  const chartPath =
    apiBase && src.startsWith(apiBase) ? src.slice(apiBase.length) : src;
  const pending = phase !== "ready";

  return (
    <figure className="border border-rule-strong bg-paper">
      {caption && (
        <figcaption className="flex min-h-11 items-center justify-between gap-3 border-b border-rule px-4 py-2">
          <span className={FIELD}>{caption}</span>
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

        {/* Only while the server is still drawing. It covers the frame rather
            than dimming it, because a half-loaded image under a veil reads as a
            rendering fault. The note is prose; the box behind it is the sheet's
            registration marks, which say "a drawing belongs here". */}
        {pending && (
          <div
            className="absolute inset-0 grid place-items-center bg-paper px-6 py-10"
            aria-live="polite"
          >
            <div className="registered w-full max-w-sm px-5 py-8 text-center">
              <p className={CAPTION}>
                {phase === "gathering"
                  ? "Reading the repository's history. This drawing appears as soon as it is rendered."
                  : "Analysis is still running. This drawing appears when it finishes."}
              </p>
            </div>
          </div>
        )}
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
