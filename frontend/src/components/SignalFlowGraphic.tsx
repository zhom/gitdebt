import { useRef, useState } from "react";
import { motion, useInView, useReducedMotion } from "motion/react";
import { RotateCcw } from "lucide-react";

import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

const STEPS = [
  {
    source: "Star timestamps",
    result: "Growth over time",
    detail: "Calendar and equal-start views",
  },
  {
    source: "Commit history",
    result: "Maintenance pressure",
    detail: "Churn, bug magnets, bus factor",
  },
  {
    source: "Package registries",
    result: "Adoption signal",
    detail: "Downloads beside attention",
  },
];

export function SignalFlowGraphic() {
  const figureRef = useRef<HTMLElement>(null);
  const inView = useInView(figureRef, { once: true, margin: "-15% 0px" });
  const reduceMotion = useReducedMotion();
  const [run, setRun] = useState(0);
  const active = reduceMotion || inView;

  return (
    <figure
      ref={figureRef}
      className="w-full min-w-0 border-y border-black bg-white text-black"
      aria-labelledby="signal-flow-caption"
    >
      <figcaption
        id="signal-flow-caption"
        className="flex min-h-14 items-center justify-between gap-4 border-b border-zinc-200 px-4 sm:px-5"
      >
        <p className="font-mono text-xs tracking-wide text-zinc-600 uppercase">
          What one repository becomes
        </p>
        <button
          type="button"
          onClick={() => setRun((value) => value + 1)}
          className="inline-flex min-h-11 items-center gap-2 text-sm text-zinc-600 outline-none hover:text-black focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-black sm:min-h-0"
        >
          Replay
          <RotateCcw className="size-3.5" strokeWidth={1.75} aria-hidden="true" />
        </button>
      </figcaption>

      <div className="px-4 py-6 sm:px-5 sm:py-8">
        <div className="flex items-center justify-between gap-4 border-b border-black pb-4">
          <p className="font-mono text-sm">facebook/react</p>
          <p className="font-mono text-xs text-zinc-500">public inputs</p>
        </div>

        <div key={run} className="divide-y divide-zinc-200">
          {STEPS.map((step, index) => {
            const delay = reduceMotion ? 0 : index * 0.12;
            return (
              <div
                key={step.source}
                className="grid gap-3 py-5 sm:grid-cols-[0.8fr_4rem_1.2fr] sm:items-center"
              >
                <div>
                  <p className="font-mono text-xs text-zinc-500">
                    0{index + 1}
                  </p>
                  <p className="mt-1 text-sm font-medium">{step.source}</p>
                </div>

                <div className="relative hidden h-px overflow-hidden bg-zinc-200 sm:block">
                  <motion.span
                    initial={{ scaleX: reduceMotion ? 1 : 0 }}
                    animate={{ scaleX: active ? 1 : 0 }}
                    transition={{
                      duration: reduceMotion
                        ? REDUCED_MOTION_DURATION
                        : DURATION.move + 0.16,
                      delay,
                      ease: EASE_OUT,
                    }}
                    className="absolute inset-0 origin-left bg-black"
                    aria-hidden="true"
                  />
                </div>

                <motion.div
                  initial={
                    run > 0
                      ? {
                          opacity: reduceMotion ? 1 : 0,
                          x: reduceMotion ? 0 : -6,
                        }
                      : false
                  }
                  animate={{
                    opacity: 1,
                    x: 0,
                  }}
                  transition={{
                    duration: reduceMotion
                      ? REDUCED_MOTION_DURATION
                      : DURATION.enter + 0.08,
                    delay: delay + (reduceMotion ? 0 : 0.14),
                    ease: EASE_OUT,
                  }}
                  className="border-l border-black pl-3 sm:border-l-0 sm:pl-0"
                >
                  <p className="text-sm font-medium">{step.result}</p>
                  <p className="mt-1 text-sm text-zinc-500">{step.detail}</p>
                </motion.div>
              </div>
            );
          })}
        </div>

        <motion.div
          key={`output-${run}`}
          initial={
            run > 0
              ? { opacity: reduceMotion ? 1 : 0, y: reduceMotion ? 0 : 6 }
              : false
          }
          animate={{ opacity: 1, y: 0 }}
          transition={{
            duration: reduceMotion
              ? REDUCED_MOTION_DURATION
              : DURATION.enter + 0.08,
            delay: reduceMotion ? 0 : 0.5,
            ease: EASE_OUT,
          }}
          className="flex flex-col justify-between gap-2 border-t border-black pt-4 sm:flex-row sm:items-center"
        >
          <p className="font-medium">One public report</p>
          <p className="font-mono text-xs text-zinc-500">
            web · SVG · GIF · extension
          </p>
        </motion.div>
      </div>
    </figure>
  );
}
