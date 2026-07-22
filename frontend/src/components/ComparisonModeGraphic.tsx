import { useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import {
  DURATION,
  EASE_IN_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

type Mode = "calendar" | "timeline";

const PATHS: Record<Mode, { first: string; second: string; ticks: string[] }> = {
  calendar: {
    first: "M20 176 C88 168 118 151 166 133 C232 108 260 75 332 46",
    second: "M20 188 C91 184 133 174 184 153 C249 127 283 109 332 88",
    ticks: ["2016", "2020", "2024"],
  },
  timeline: {
    first: "M20 183 C71 177 104 161 148 132 C216 88 262 68 332 48",
    second: "M20 183 C76 172 107 140 155 111 C211 78 276 72 332 65",
    ticks: ["Day 0", "Year 4", "Year 8"],
  },
};

export function ComparisonModeGraphic() {
  const [mode, setMode] = useState<Mode>("calendar");
  const reduceMotion = useReducedMotion();
  const active = PATHS[mode];

  return (
    <figure
      className="border-y border-foreground bg-card text-card-foreground"
      aria-labelledby="comparison-graphic-caption"
    >
      <div className="flex flex-col justify-between gap-4 border-b border-border px-4 py-4 sm:flex-row sm:items-center sm:px-5">
        <figcaption id="comparison-graphic-caption">
          <p className="font-medium">Change the question, not the data</p>
          <p className="mt-1 text-sm text-muted-foreground">
            Illustrative axes — no repository values
          </p>
        </figcaption>
        <div className="flex border-b border-border" aria-label="Comparison axis">
          {(["calendar", "timeline"] as const).map((option) => (
            <button
              key={option}
              type="button"
              onClick={() => setMode(option)}
              aria-pressed={mode === option}
              className="relative min-h-11 px-3 font-mono text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring aria-pressed:text-foreground sm:min-h-9"
            >
              {option === "calendar" ? "Calendar date" : "Equal start"}
              {mode === option && (
                <motion.span
                  layoutId="comparison-mode-underline"
                  transition={{
                    duration: reduceMotion
                      ? REDUCED_MOTION_DURATION
                      : DURATION.move,
                    ease: EASE_IN_OUT,
                  }}
                  className="absolute inset-x-0 -bottom-px h-px bg-foreground"
                  aria-hidden="true"
                />
              )}
            </button>
          ))}
        </div>
      </div>

      <div className="px-4 py-5 sm:px-5">
        <svg
          viewBox="0 0 352 220"
          role="img"
          aria-label={
            mode === "calendar"
              ? "Two illustrative repository trajectories aligned by calendar date"
              : "Two illustrative repository trajectories aligned by time since first star"
          }
          className="block h-auto w-full"
        >
          <path d="M20 24V188H336" fill="none" stroke="var(--border)" />
          {[76, 132].map((y) => (
            <path
              key={y}
              d={`M20 ${y}H336`}
              fill="none"
              stroke="var(--border)"
              strokeDasharray="3 5"
            />
          ))}
          <motion.path
            d={active.first}
            animate={{ d: active.first }}
            transition={{
              duration: reduceMotion
                ? REDUCED_MOTION_DURATION
                : DURATION.chart + 0.1,
              ease: EASE_IN_OUT,
            }}
            fill="none"
            stroke="var(--foreground)"
            strokeLinecap="round"
            strokeWidth="2.5"
          />
          <motion.path
            d={active.second}
            animate={{ d: active.second }}
            transition={{
              duration: reduceMotion
                ? REDUCED_MOTION_DURATION
                : DURATION.chart + 0.1,
              ease: EASE_IN_OUT,
            }}
            fill="none"
            stroke="var(--muted-foreground)"
            strokeDasharray="5 5"
            strokeLinecap="round"
            strokeWidth="2"
          />
          {active.ticks.map((tick, index) => (
            <motion.text
              key={`${mode}-${tick}`}
              initial={{ opacity: reduceMotion ? 1 : 0 }}
              animate={{ opacity: 1 }}
              transition={{
                duration: reduceMotion
                  ? REDUCED_MOTION_DURATION
                  : DURATION.enter,
                delay: reduceMotion ? 0 : index * 0.04,
              }}
              x={[20, 176, 336][index]}
              y="210"
              fill="var(--muted-foreground)"
              fontFamily="ui-monospace, monospace"
              fontSize="9"
              textAnchor={index === 0 ? "start" : index === 2 ? "end" : "middle"}
            >
              {tick}
            </motion.text>
          ))}
        </svg>

        <div className="flex flex-wrap gap-x-6 gap-y-2 border-t border-border pt-4 text-sm">
          <span className="inline-flex items-center gap-2">
            <span className="h-0.5 w-5 bg-foreground" aria-hidden="true" />
            Repository A
          </span>
          <span className="inline-flex items-center gap-2 text-muted-foreground">
            <span className="w-5 border-t border-dashed border-muted-foreground" aria-hidden="true" />
            Repository B
          </span>
        </div>
      </div>
    </figure>
  );
}
