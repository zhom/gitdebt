import { useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";

import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

type Theme = "light" | "dark";

export function ThemeEmbedPreview() {
  const [theme, setTheme] = useState<Theme>("light");
  const reduceMotion = useReducedMotion();
  const dark = theme === "dark";

  return (
    <figure
      className="border-y border-foreground bg-card text-card-foreground"
      aria-labelledby="theme-preview-caption"
    >
      <div className="flex flex-col justify-between gap-4 border-b border-border px-4 py-4 sm:flex-row sm:items-center sm:px-5">
        <figcaption id="theme-preview-caption">
          <p className="font-medium">One snippet, both README themes</p>
          <p className="mt-1 text-sm text-muted-foreground">
            The matching asset is selected by GitHub
          </p>
        </figcaption>
        <div className="flex border-b border-border" aria-label="README theme preview">
          {(["light", "dark"] as const).map((option) => (
            <button
              key={option}
              type="button"
              onClick={() => setTheme(option)}
              aria-pressed={theme === option}
              className="relative min-h-11 px-3 font-mono text-xs text-muted-foreground outline-none hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring aria-pressed:text-foreground sm:min-h-9"
            >
              {option === "light" ? "Light README" : "Dark README"}
              {theme === option && (
                <motion.span
                  layoutId="theme-preview-underline"
                  transition={{
                    duration: reduceMotion
                      ? REDUCED_MOTION_DURATION
                      : DURATION.move,
                    ease: EASE_OUT,
                  }}
                  className="absolute inset-x-0 -bottom-px h-px bg-foreground"
                  aria-hidden="true"
                />
              )}
            </button>
          ))}
        </div>
      </div>

      <div className="p-4 sm:p-5">
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
            className={`min-h-52 border p-5 ${
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
                <path
                  d="M2 55 C48 54 74 43 104 41 C157 37 181 24 221 19 C248 15 265 10 278 4"
                  fill="none"
                  stroke="currentColor"
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
