import { useId } from "react";
import { motion, useReducedMotion } from "motion/react";

import { DitherCellPattern } from "@/components/DitherCellPattern";
import { useInView } from "@/components/ui/use-in-view";
import { SWATCH } from "@/lib/dither";

const SERIES = `rgb(${SWATCH.blue.join(", ")})`;

type Kind = "stars" | "health" | "readme";

/**
 * Five looping animations, so it needs both halves of the motion law.
 *
 * `client:visible` gates the MOUNT and nothing else: once this has been
 * scrolled past it would keep running for the rest of the session. `useInView`
 * is what actually stops it, in both directions and on `visibilitychange`.
 * When inactive — reduced motion, off-screen, or a hidden tab — every element
 * settles to the same single deterministic frame rather than freezing wherever
 * the loop happened to be.
 */
export function ReportLayerGraphic({ kind }: { kind: Kind }) {
  const reduceMotion = useReducedMotion();
  const [ref, inView] = useInView<SVGSVGElement>();
  const active = !reduceMotion && inView;
  const id = useId().replaceAll(":", "");
  const duration = active ? 4.8 : 0;
  const repeat = active ? Infinity : 0;

  return (
    <svg ref={ref} viewBox="0 0 176 72" role="img" aria-label={`${kind} data in motion`} className="block h-16 w-40 max-w-full overflow-visible">
      <defs>
        <DitherCellPattern id={`${id}-cells`} fill={SERIES} density={0.55} />
        <clipPath id={`${id}-clip`}><rect width="176" height="72" /></clipPath>
      </defs>
      <rect y="1" width="176" height="70" fill="none" stroke="var(--border)" />

      {kind === "stars" && <g clipPath={`url(#${id}-clip)`}>
        <motion.path
          d="M0 64 C26 61 34 49 55 50 S84 39 105 36 S137 22 176 10 V72 H0Z"
          fill={`url(#${id}-cells)`}
          animate={active ? { x: [0, 3, 0], y: [0, -2, 0] } : { x: 0, y: 0 }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
        <motion.path
          d="M0 64 C26 61 34 49 55 50 S84 39 105 36 S137 22 176 10"
          fill="none" stroke={SERIES} strokeWidth="1.5"
          animate={active ? { pathLength: [0.82, 1, 0.9], opacity: [0.7, 1, 0.78] } : { pathLength: 1, opacity: 1 }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
      </g>}

      {kind === "health" && <g>
        {[18, 36, 54].map((y, index) => (
          <g key={y}>
            <rect x="12" y={y - 4} width="150" height="8" fill="var(--muted)" />
            <motion.rect
              x="12" y={y - 4} height="8" fill={`url(#${id}-cells)`}
              animate={active ? { width: [48 + index * 12, 116 - index * 9, 62 + index * 15] } : { width: 72 + index * 18 }}
              transition={{ duration: duration + (active ? index * 0.6 : 0), repeat, ease: "easeInOut", delay: active ? index * 0.18 : 0 }}
            />
          </g>
        ))}
      </g>}

      {kind === "readme" && <g>
        <path d="M18 17h44M18 28h71M18 39h58" stroke="var(--muted-foreground)" strokeWidth="2" />
        <rect x="18" y="49" width="140" height="11" fill={`url(#${id}-cells)`} />
        <motion.path
          d="M103 17l-6 6 6 6M113 17l6 6-6 6"
          fill="none" stroke={SERIES} strokeWidth="2"
          animate={active ? { x: [0, 9, 0], opacity: [0.6, 1, 0.6] } : { x: 0, opacity: 1 }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
        <motion.rect
          x="18" y="49" width="22" height="11" fill={SERIES} opacity=".45"
          animate={active ? { x: [0, 118, 0] } : { x: 0 }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
      </g>}
    </svg>
  );
}
