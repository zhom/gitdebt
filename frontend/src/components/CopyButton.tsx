import { useEffect, useRef, useState, type ReactNode } from "react";

import { Button, type ButtonProps } from "@/components/ui/button";
import { Leader, Tick } from "@/components/ui/marks";
import { cn } from "@/lib/utils";

/**
 * The copy action.
 *
 * It confirms by INK, not by movement: nothing lifts, nothing scales, no glyph
 * springs in from a smaller copy of itself. The label steps from `ink-2` to
 * full graphite and the draughtsman's tick strikes itself along its own length.
 * That is a state change you can see in a still frame, which is the only kind
 * this site ships.
 *
 * The idle mark is the leader arrow, up and out: this action takes the snippet
 * off the page and into somebody's README. There is no clipboard glyph in
 * `ui/marks.tsx` and one was not invented for this — the house set has five
 * marks and the leader is the one that means "out to there".
 */

type Props = {
  value: string;
  ariaLabel: string;
  className?: string;
  idleLabel?: string;
  successLabel?: string;
  /** Replaces the leader when the copy means something more specific. */
  idleIcon?: ReactNode;
  variant?: ButtonProps["variant"];
  size?: ButtonProps["size"];
};

/** How long the confirmation holds before the label returns to its idle ink. */
const REVERT_MS = 2000;

/**
 * Measured length of the `Tick` path — `M3 8.5 6.5 12 13 4.5` — summed here
 * rather than read from the DOM, so the stroke draws to its true end on the
 * first frame instead of stopping short of it.
 */
const TICK_LENGTH = Math.ceil(Math.hypot(3.5, 3.5) + Math.hypot(6.5, 7.5));

/** A confirmation strikes; it does not travel across the sheet. */
const TICK_DURATION = "260ms";

export function CopyButton({
  value,
  ariaLabel,
  className,
  idleLabel = "Copy",
  successLabel = "Copied",
  idleIcon,
  variant = "quiet",
  size = "default",
}: Props) {
  const [copied, setCopied] = useState(false);
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

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

  return (
    <Button
      variant={variant}
      size={size}
      onClick={copy}
      aria-label={ariaLabel}
      className={cn(
        // The confirmation, in ink. `primary` is already at full contrast, so
        // it keeps its own treatment and confirms with the tick alone.
        copied && variant !== "primary" && "text-ink",
        copied && variant === "quiet" && "border-ink-3",
        className,
      )}
    >
      <span className="grid size-3.5 shrink-0 place-items-center">
        {copied ? (
          <Tick
            size={14}
            className="col-start-1 row-start-1 inks-in"
            style={{
              ["--draw-length" as string]: String(TICK_LENGTH),
              ["--duration-draw" as string]: TICK_DURATION,
            }}
          />
        ) : (
          <span className="col-start-1 row-start-1 inline-flex">
            {idleIcon ?? <Leader size={14} />}
          </span>
        )}
      </span>

      <span className="grid">
        {/* Sized by the wider of the two labels, so the row never reflows when
            the word changes under the pointer. */}
        <span className="invisible col-start-1 row-start-1" aria-hidden="true">
          {idleLabel}
        </span>
        <span className="invisible col-start-1 row-start-1" aria-hidden="true">
          {successLabel}
        </span>
        <span className="col-start-1 row-start-1 text-left" aria-hidden="true">
          {copied ? successLabel : idleLabel}
        </span>
      </span>

      <span className="sr-only" aria-live="polite">
        {copied ? successLabel : ""}
      </span>
    </Button>
  );
}
