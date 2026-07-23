import { useEffect, useRef, useState } from "react";
import { motion, useReducedMotion } from "motion/react";
import { Check, Copy } from "lucide-react";

import { Button, type ButtonProps } from "@/components/ui/button";
import { SPRING } from "@/lib/motion";

type Props = {
  value: string;
  ariaLabel: string;
  className?: string;
  idleLabel?: string;
  successLabel?: string;
  variant?: ButtonProps["variant"];
  size?: ButtonProps["size"];
};

/** How long the confirmation holds before the glyph springs back. */
const REVERT_MS = 2000;

export function CopyButton({
  value,
  ariaLabel,
  className,
  idleLabel = "Copy",
  successLabel = "Copied",
  variant = "soft",
  size = "sm",
}: Props) {
  const [copied, setCopied] = useState(false);
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
      setCopied(true);
      timerRef.current = setTimeout(() => setCopied(false), REVERT_MS);
    } catch {
      setCopied(false);
    }
  }

  // Both glyphs stay mounted, so a second copy mid-flight retargets the spring
  // from wherever the icons currently sit instead of restarting from scratch.
  const transition = reduceMotion ? { duration: 0 } : SPRING.snappy;
  const glyph = (active: boolean) => ({
    scale: reduceMotion ? 1 : active ? 1 : 0.4,
    opacity: active ? 1 : 0,
  });

  return (
    <Button
      variant={variant}
      size={size}
      onClick={copy}
      aria-label={ariaLabel}
      className={className}
    >
      <span className="grid size-3.5 shrink-0 place-items-center">
        <motion.span
          className="col-start-1 row-start-1 inline-flex"
          initial={false}
          animate={glyph(!copied)}
          transition={transition}
          aria-hidden="true"
        >
          <Copy className="size-3.5" strokeWidth={2} />
        </motion.span>
        <motion.span
          className="col-start-1 row-start-1 inline-flex"
          initial={false}
          animate={glyph(copied)}
          transition={transition}
          aria-hidden="true"
        >
          <Check className="size-3.5" strokeWidth={2} />
        </motion.span>
      </span>
      <span className="grid">
        {/* Sized by the wider label, so the button never jumps mid-swap. */}
        <span className="invisible col-start-1 row-start-1" aria-hidden="true">
          {idleLabel}
        </span>
        <span className="invisible col-start-1 row-start-1" aria-hidden="true">
          {successLabel}
        </span>
        <motion.span
          className="col-start-1 row-start-1 text-left"
          initial={false}
          animate={{ opacity: copied ? 0 : 1 }}
          transition={transition}
          aria-hidden="true"
        >
          {idleLabel}
        </motion.span>
        <motion.span
          className="col-start-1 row-start-1 text-left"
          initial={false}
          animate={{ opacity: copied ? 1 : 0 }}
          transition={transition}
          aria-hidden="true"
        >
          {successLabel}
        </motion.span>
      </span>
      <span className="sr-only" aria-live="polite">
        {copied ? successLabel : ""}
      </span>
    </Button>
  );
}
