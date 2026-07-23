import { useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import { DitherCellPattern } from "@/components/DitherCellPattern";
import { CAPTION, PANEL } from "@/components/style-tokens";
import { DitherSegmented } from "@/components/ui/dither-segmented";
import { SWATCH } from "@/lib/dither";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";
import { cn } from "@/lib/utils";

const SERIES = `rgb(${SWATCH.blue.join(", ")})`;

const THEMES = [
  { value: "light" as const, label: "Light README" },
  { value: "dark" as const, label: "Dark README" },
];

type Theme = "light" | "dark";

export function ThemeEmbedPreview() {
  const [theme, setTheme] = useState<Theme>("light");
  const reduceMotion = useReducedMotion();
  const dark = theme === "dark";

  return (
    <figure
      className={cn(PANEL, "overflow-hidden")}
      aria-labelledby="theme-preview-caption"
    >
      <div className="flex flex-col justify-between gap-3 border-b border-border/40 p-3.5 sm:flex-row sm:items-center">
        <figcaption id="theme-preview-caption">
          <p className="text-[13px]">One snippet, both README themes</p>
          <p className={cn(CAPTION, "mt-1")}>
            The matching asset is selected by GitHub
          </p>
        </figcaption>
        <DitherSegmented
          role="radiogroup"
          aria-label="README theme preview"
          value={theme}
          options={THEMES}
          onValueChange={setTheme}
        />
      </div>

      <div className="p-3.5">
        <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={theme}
            initial={{
              opacity: reduceMotion ? 1 : 0,
              scale: reduceMotion ? 1 : 0.995,
            }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0 }}
            transition={{
              duration: reduceMotion
                ? REDUCED_MOTION_DURATION
                : DURATION.enter,
              ease: EASE_OUT,
            }}
            className={`min-h-52 rounded-lg border p-5 ${
              dark
                ? "border-zinc-800 bg-black text-white"
                : "border-zinc-200 bg-white text-black"
            }`}
          >
            <p className={`font-mono text-xs ${dark ? "text-zinc-400" : "text-zinc-500"}`}>
              README.md
            </p>
            <div className={`mt-8 border-y py-5 ${dark ? "border-zinc-700" : "border-zinc-300"}`}>
              <div className="flex items-center justify-between gap-4">
                <p className="font-mono text-sm">facebook/react</p>
                <span className={`font-mono text-xs ${dark ? "text-zinc-400" : "text-zinc-500"}`}>
                  star history
                </span>
              </div>
              <svg
                viewBox="0 0 280 64"
                className="mt-4 block w-full"
                aria-label={`Illustrative ${theme} README chart`}
                role="img"
              >
                <defs>
                  <DitherCellPattern
                    id={`readme-cells-${theme}`}
                    fill={SERIES}
                    density={0.55}
                  />
                </defs>
                <motion.path
                  d="M2 55 C48 54 74 43 104 41 C157 37 181 24 221 19 C248 15 265 10 278 4 V64 H2Z"
                  fill={`url(#readme-cells-${theme})`}
                  animate={reduceMotion ? undefined : { y: [0, -1.5, 0] }}
                  transition={{ duration: 4.5, repeat: Infinity, ease: "easeInOut" }}
                />
                <path
                  d="M2 55 C48 54 74 43 104 41 C157 37 181 24 221 19 C248 15 265 10 278 4"
                  fill="none"
                  stroke={SERIES}
                  strokeLinecap="round"
                  strokeWidth="2"
                />
              </svg>
            </div>
          </motion.div>
        </AnimatePresence>
      </div>
    </figure>
  );
}
