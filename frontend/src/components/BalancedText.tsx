import { createElement, useEffect, useRef, type Ref } from "react";

import { balancedLayout, isBrowser, metricsFor } from "@/lib/pretext";

type Props = {
  /** Plain text only — this measures and re-wraps the string itself. */
  children: string;
  as?: "h1" | "h2" | "h3" | "p" | "div";
  className?: string;
  /**
   * Pin the measured height as `min-height` so a later, differently-sized
   * value dropped into the same slot cannot shift the page. On by default —
   * it is the reason to reach for this over CSS `text-wrap: balance`, which
   * balances the rag but reserves nothing.
   */
  reserveHeight?: boolean;
};

/**
 * A block of text balanced to the tightest width that keeps its line count,
 * with its height reserved — both computed by pretext off the DOM.
 *
 * The server-rendered markup is the plain string (CSS `text-wrap: balance`
 * carries the pre-hydration look), then on the client pretext refines the rag
 * to the pixel and locks the height. Intended for headings whose text arrives
 * or changes after first paint; feed it a single string.
 */
export function BalancedText({
  children,
  as = "h2",
  className,
  reserveHeight = true,
}: Props) {
  const ref = useRef<HTMLElement>(null);

  useEffect(() => {
    const el = ref.current;
    if (!el || !isBrowser() || !children) return;

    let frame = 0;
    const apply = () => {
      // Clear our own constraints first so the read sees the width the text
      // is actually allowed (respecting any `max-w-*` class still in force).
      el.style.maxWidth = "";
      if (reserveHeight) el.style.minHeight = "";
      const avail = el.getBoundingClientRect().width;
      if (avail <= 0) return;
      const { font, lineHeight, letterSpacing } = metricsFor(el);
      const { width, height } = balancedLayout(
        children,
        font,
        avail,
        lineHeight,
        letterSpacing,
      );
      el.style.maxWidth = `${width}px`;
      if (reserveHeight) el.style.minHeight = `${height}px`;
    };

    frame = requestAnimationFrame(apply);
    // Re-balance when the container (hence the available width, and any
    // responsive font-size) changes.
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(apply);
    });
    const target = el.parentElement ?? el;
    observer.observe(target);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [children, reserveHeight]);

  return createElement(
    as,
    { ref: ref as Ref<HTMLElement>, className },
    children,
  );
}
