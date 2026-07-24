"use client";

import * as React from "react";

import { BRAND, SWATCH, type RGB } from "@/lib/dither";
import { buttonVariants } from "@/components/ui/button";
import { useDitherSurface } from "@/components/ui/dither-surface";

type ButtonVariants = Omit<
  NonNullable<Parameters<typeof buttonVariants>[0]>,
  "class" | "className"
>;

const CANVAS_FILL: Record<string, RGB> = {
  default: BRAND,
  primary: BRAND,
  accent: SWATCH.blue,
  soft: BRAND,
};

const CANVAS_ALPHA: Record<string, number> = { soft: 0.42 };

export type ButtonLinkProps = React.AnchorHTMLAttributes<HTMLAnchorElement> &
  ButtonVariants & {
    /**
     * Explicitly gives a quiet link the same cursor-positioned dither pulse as
     * a primary action, without adding a filled resting surface.
     */
    pulse?: boolean;
  };

/**
 * An anchor wearing the button treatment. Navigation stays an `<a>`; textured
 * variants mount a filled canvas, while an opted-in quiet action mounts only
 * the transparent pulse layer.
 */
export const ButtonLink = React.forwardRef<HTMLAnchorElement, ButtonLinkProps>(
  ({ className, variant, size, pulse, children, ...props }, ref) => {
    const fill = CANVAS_FILL[variant ?? "default"];
    const textured = fill !== undefined;
    const pulseEnabled = pulse ?? textured;
    const surfaceEnabled = textured || pulseEnabled;
    const { surface, handlers } = useDitherSurface({
      fill: fill ?? SWATCH.blue,
      variant: "gradient",
      animated: surfaceEnabled,
      alpha: textured ? CANVAS_ALPHA[variant ?? "default"] : 0,
      pulse: pulseEnabled,
    });
    return (
      <a
        ref={ref}
        data-press="off"
        className={buttonVariants({ variant, size, className })}
        {...props}
        {...(surfaceEnabled ? handlers : {})}
      >
        {surfaceEnabled ? surface : null}
        <span className="relative inline-flex items-center gap-2">
          {children}
        </span>
      </a>
    );
  },
);
ButtonLink.displayName = "ButtonLink";
