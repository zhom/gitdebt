"use client";

import { useEffect, useRef, useState, type RefObject } from "react";

/**
 * "Is this surface worth painting right now?"
 *
 * The motion law has two halves and every animated surface needs both: stop
 * when the element scrolls out of view, and stop when the document is hidden.
 * `client:visible` only gates the MOUNT — once an island has been scrolled
 * past, it keeps running its loop for the rest of the session unless something
 * turns it off, which is exactly the defect this hook exists to remove.
 *
 * Transcribed from `MomentumBoard.tsx`, including the part that is easy to get
 * wrong: the state is recomputed in BOTH directions. Stopping on `hidden`
 * without restarting on `visible` froze the board permanently, because the
 * intersection never changed and so the observer never fired again.
 *
 * Attach the ref to an element that exists on the surface's first render. A
 * conditionally-rendered animated block should live in its own component and
 * call this hook there, so mounting the block is what starts the observer.
 */
export function useInView<T extends Element>(
  options: { rootMargin?: string } = {},
): [RefObject<T | null>, boolean] {
  const rootMargin = options.rootMargin ?? "128px";
  const ref = useRef<T>(null);
  const [inView, setInView] = useState(false);

  useEffect(() => {
    const node = ref.current;
    if (!node) return;

    // No observer (very old engines, some test runners): fall back to the
    // document's own visibility rather than pinning the surface off forever.
    if (typeof IntersectionObserver !== "function") {
      setInView(!document.hidden);
      return;
    }

    let onScreen = false;
    const sync = () => setInView(onScreen && !document.hidden);

    const observer = new IntersectionObserver(
      ([entry]) => {
        onScreen = entry?.isIntersecting ?? false;
        sync();
      },
      { rootMargin },
    );
    observer.observe(node);
    document.addEventListener("visibilitychange", sync);

    return () => {
      observer.disconnect();
      document.removeEventListener("visibilitychange", sync);
    };
  }, [rootMargin]);

  return [ref, inView];
}
