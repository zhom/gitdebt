import { useEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Check, Copy } from "lucide-react";

import { Button, type ButtonProps } from "@/components/ui/button";
import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

type Props = {
  value: string;
  ariaLabel: string;
  className?: string;
  idleLabel?: string;
  successLabel?: string;
  variant?: ButtonProps["variant"];
  size?: ButtonProps["size"];
};

export function CopyButton({
  value,
  ariaLabel,
  className,
  idleLabel = "Copy",
  successLabel = "Copied",
  variant = "outline",
  size = "sm",
}: Props) {
  const [copied, setCopied] = useState(false);
  const [feedbackKey, setFeedbackKey] = useState(0);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const reduceMotion = useReducedMotion();

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current);
    },
    [],
  );

  async function copy() {
    try {
      await navigator.clipboard.writeText(value);
      if (timerRef.current) clearTimeout(timerRef.current);
      setFeedbackKey((key) => key + 1);
      setCopied(true);
      timerRef.current = setTimeout(() => setCopied(false), 1600);
    } catch {
      setCopied(false);
    }
  }

  const duration = reduceMotion
    ? REDUCED_MOTION_DURATION
    : DURATION.feedback;

  return (
    <Button
      variant={variant}
      size={size}
      onClick={copy}
      aria-label={ariaLabel}
      className={className}
    >
      <span className="grid items-center">
        <span
          className="invisible col-start-1 row-start-1 inline-flex items-center gap-1.5"
          aria-hidden="true"
        >
          <Check className="size-3.5 shrink-0" />
          {successLabel}
        </span>
        <span
          className="invisible col-start-1 row-start-1 inline-flex items-center gap-1.5"
          aria-hidden="true"
        >
          <Copy className="size-3.5 shrink-0" />
          {idleLabel}
        </span>
        <AnimatePresence initial={false} mode="popLayout">
          <motion.span
            key={`${copied ? "copied" : "idle"}-${feedbackKey}`}
            initial={{
              opacity: 0,
              scale: reduceMotion ? 1 : 0.97,
            }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{
              opacity: 0,
              scale: reduceMotion ? 1 : 0.97,
              transition: {
                duration: reduceMotion ? REDUCED_MOTION_DURATION : 0.1,
                ease: EASE_OUT,
              },
            }}
            transition={{ duration, ease: EASE_OUT }}
            className="col-start-1 row-start-1 inline-flex items-center gap-1.5"
            aria-live="polite"
          >
            {copied ? (
              <Check
                className="size-3.5 shrink-0"
                strokeWidth={2}
                aria-hidden="true"
              />
            ) : (
              <Copy
                className="size-3.5 shrink-0"
                strokeWidth={2}
                aria-hidden="true"
              />
            )}
            {copied ? successLabel : idleLabel}
          </motion.span>
        </AnimatePresence>
      </span>
    </Button>
  );
}
