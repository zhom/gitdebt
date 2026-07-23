import { useId, useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import { DitherCellPattern } from "@/components/DitherCellPattern";
import { CAPTION, PANEL } from "@/components/style-tokens";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import { SWATCH } from "@/lib/dither";
import { DURATION, EASE_IN_OUT, REDUCED_MOTION_DURATION } from "@/lib/motion";
import { cn } from "@/lib/utils";

const SERIES = `rgb(${SWATCH.blue.join(", ")})`;

const MODES = [
  { value: "calendar" as const, label: "Calendar date" },
  { value: "timeline" as const, label: "Equal start" },
];

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
  const id = useId().replaceAll(":", "");
  const firstArea = `${active.first} V188 H20 Z`;

  return (
    <figure
      className={cn(PANEL, "overflow-hidden")}
      aria-labelledby="comparison-graphic-caption"
    >
      <div className="flex flex-col justify-between gap-3 border-b border-border/40 p-3.5 sm:flex-row sm:items-center">
        <figcaption id="comparison-graphic-caption">
          <p className="text-[13px]">Change the question, not the data</p>
          <p className={cn(CAPTION, "mt-1")}>
            Illustrative axes — no repository values
          </p>
        </figcaption>
        <DitherSegmented
          role="radiogroup"
          aria-label="Comparison axis"
          value={mode}
          options={MODES}
          onValueChange={setMode}
        />
      </div>

      <div className="p-3.5">
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
          <defs>
            <DitherCellPattern id={`${id}-cells`} fill={SERIES} density={0.55} />
          </defs>
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
            d={firstArea}
            animate={{ d: firstArea }}
            transition={{
              duration: reduceMotion
                ? REDUCED_MOTION_DURATION
                : DURATION.chart + 0.1,
              ease: EASE_IN_OUT,
            }}
            fill={`url(#${id}-cells)`}
          />
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
            stroke={SERIES}
            strokeLinecap="round"
            strokeWidth="2"
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

        <div className="flex flex-wrap gap-x-6 gap-y-2 border-t border-border/40 pt-4 text-[11px]">
          <span className="inline-flex items-center gap-2">
            <span
              className="size-2 rounded-[1px]"
              style={{ backgroundColor: SERIES }}
              aria-hidden="true"
            />
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
