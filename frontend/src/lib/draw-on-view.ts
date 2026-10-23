/**
 * A drawing below the fold draws itself when the reader reaches it.
 *
 * Every drawing on the site inks in on page load, which is correct for the
 * first viewport and wasted on everything under it: by the time a reader
 * scrolls to a chart, its pen finished long ago off-screen. This puts the draw
 * where the reader is, without breaking the rule that outranks motion — a
 * drawing is complete before it moves.
 *
 * So nothing is hidden up front. A `[data-draw]` drawing is only rewound once
 * a person has actually scrolled (wheel, touch or keyboard; a crawler, a
 * screenshot pass and a framed preview never do), and only while it is wholly
 * outside the viewport, so the rewind itself is never seen. When it comes into
 * view its own CSS animations play again from the start, with every delay and
 * stagger they were written with. Reduced motion never rewinds anything, and
 * printing finishes whatever is still waiting.
 *
 * Only the drawing keyframes are rewound. A hover transition or a spinner in
 * the same subtree is left alone.
 */

const DRAW_KEYFRAMES = new Set([
  "draw-stroke",
  "extend-x",
  "extend-y",
  "measure-in",
  "land",
  "settle",
  "pen-travel",
]);

const SELECTOR = "[data-draw]";

function drawingAnimations(root: Element): CSSAnimation[] {
  return root
    .getAnimations({ subtree: true })
    .filter(
      (animation): animation is CSSAnimation =>
        typeof CSSAnimation !== "undefined" &&
        animation instanceof CSSAnimation &&
        DRAW_KEYFRAMES.has(animation.animationName),
    );
}

export function installDrawOnView(): void {
  if (
    typeof IntersectionObserver === "undefined" ||
    typeof Element.prototype.getAnimations !== "function" ||
    window.matchMedia("(prefers-reduced-motion: reduce)").matches
  ) {
    return;
  }

  /** Drawings rewound and waiting to be seen, with what was rewound. */
  const armed = new Map<Element, CSSAnimation[]>();
  /** Drawings the reader has seen, or that were in view when checked. */
  const settled = new WeakSet<Element>();
  let reader = false;
  let pending = false;

  const play = (element: Element) => {
    const animations = armed.get(element);
    armed.delete(element);
    for (const animation of animations ?? []) {
      animation.currentTime = 0;
      animation.play();
    }
  };

  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (!entry.isIntersecting) continue;
        observer.unobserve(entry.target);
        settled.add(entry.target);
        play(entry.target);
      }
    },
    // The same boundary the sweep rewinds on: a drawing plays the moment any
    // of it is on screen, so a rewound drawing is never seen standing still,
    // and one at the very end of the page cannot be left waiting for a
    // scroll position the page does not have.
    { rootMargin: "0px" },
  );

  const sweep = () => {
    pending = false;
    const height = window.innerHeight;
    for (const element of document.querySelectorAll(SELECTOR)) {
      if (settled.has(element) || armed.has(element)) continue;
      const box = element.getBoundingClientRect();
      if (box.height === 0) continue;
      if (box.top < height && box.bottom > 0) {
        settled.add(element);
        continue;
      }
      const animations = drawingAnimations(element);
      if (animations.length === 0) continue;
      for (const animation of animations) {
        animation.pause();
        animation.currentTime = 0;
      }
      armed.set(element, animations);
      observer.observe(element);
    }
  };

  const schedule = () => {
    if (!reader || pending) return;
    pending = true;
    window.setTimeout(sweep, 120);
  };

  const wake = () => {
    if (reader) return;
    reader = true;
    schedule();
  };

  for (const type of ["wheel", "touchmove", "keydown"] as const) {
    window.addEventListener(type, wake, { passive: true, once: true });
  }
  // Capture, so a sheet that scrolls inside itself (embed mode) counts too.
  document.addEventListener("scroll", schedule, { passive: true, capture: true });

  window.addEventListener("beforeprint", () => {
    for (const animations of armed.values()) {
      for (const animation of animations) animation.finish();
    }
    armed.clear();
  });
}
