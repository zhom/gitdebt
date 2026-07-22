import { useId } from "react";
import { motion, useReducedMotion } from "motion/react";

type Kind = "stars" | "health" | "readme";

export function ReportLayerGraphic({ kind }: { kind: Kind }) {
  const reduceMotion = useReducedMotion();
  const id = useId().replaceAll(":", "");
  const duration = reduceMotion ? 0 : 4.8;
  const repeat = reduceMotion ? 0 : Infinity;

  return (
    <svg viewBox="0 0 176 72" role="img" aria-label={`${kind} data in motion`} className="block h-16 w-40 max-w-full overflow-visible">
      <defs>
        <linearGradient id={`${id}-wave`} x1="0" x2="1">
          <stop offset="0" stopColor="var(--dither-wave-1)" />
          <stop offset=".52" stopColor="var(--dither-wave-2)" />
          <stop offset="1" stopColor="var(--dither-wave-3)" />
        </linearGradient>
        <pattern id={`${id}-dots`} width="4" height="4" patternUnits="userSpaceOnUse">
          <rect width="1.4" height="1.4" fill={`url(#${id}-wave)`} />
          <rect x="2" y="2" width=".8" height=".8" fill={`url(#${id}-wave)`} opacity=".65" />
        </pattern>
        <clipPath id={`${id}-clip`}><rect width="176" height="72" /></clipPath>
      </defs>
      <rect y="1" width="176" height="70" fill="none" stroke="var(--border)" />

      {kind === "stars" && <g clipPath={`url(#${id}-clip)`}>
        <motion.path
          d="M0 64 C26 61 34 49 55 50 S84 39 105 36 S137 22 176 10 V72 H0Z"
          fill={`url(#${id}-dots)`}
          animate={reduceMotion ? undefined : { x: [0, 3, 0], y: [0, -2, 0] }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
        <motion.path
          d="M0 64 C26 61 34 49 55 50 S84 39 105 36 S137 22 176 10"
          fill="none" stroke={`url(#${id}-wave)`} strokeWidth="1.5"
          animate={reduceMotion ? undefined : { pathLength: [0.82, 1, 0.9], opacity: [0.7, 1, 0.78] }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
      </g>}

      {kind === "health" && <g>
        {[18, 36, 54].map((y, index) => (
          <g key={y}>
            <rect x="12" y={y - 4} width="150" height="8" fill="var(--muted)" />
            <motion.rect
              x="12" y={y - 4} height="8" fill={`url(#${id}-dots)`}
              animate={reduceMotion ? { width: 72 + index * 18 } : { width: [48 + index * 12, 116 - index * 9, 62 + index * 15] }}
              transition={{ duration: duration + index * 0.6, repeat, ease: "easeInOut", delay: index * 0.18 }}
            />
          </g>
        ))}
      </g>}

      {kind === "readme" && <g>
        <path d="M18 17h44M18 28h71M18 39h58" stroke="var(--muted-foreground)" strokeWidth="2" />
        <rect x="18" y="49" width="140" height="11" fill={`url(#${id}-dots)`} />
        <motion.path
          d="M103 17l-6 6 6 6M113 17l6 6-6 6"
          fill="none" stroke={`url(#${id}-wave)`} strokeWidth="2"
          animate={reduceMotion ? undefined : { x: [0, 9, 0], opacity: [0.6, 1, 0.6] }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
        <motion.rect
          x="18" y="49" width="22" height="11" fill={`url(#${id}-wave)`} opacity=".45"
          animate={reduceMotion ? undefined : { x: [0, 118, 0] }}
          transition={{ duration, repeat, ease: "easeInOut" }}
        />
      </g>}
    </svg>
  );
}
