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
};

export type ButtonLinkProps = React.AnchorHTMLAttributes<HTMLAnchorElement> &
  ButtonVariants;

/**
 * An anchor wearing the button treatment. Navigation stays an `<a>`; only the
 * textured variants mount a canvas, exactly as `<Button>` does.
 */
export const ButtonLink = React.forwardRef<HTMLAnchorElement, ButtonLinkProps>(
  ({ className, variant, size, children, ...props }, ref) => {
    const fill = CANVAS_FILL[variant ?? "default"];
    const textured = fill !== undefined;
    const { surface, handlers } = useDitherSurface({
      fill: fill ?? BRAND,
      variant: "gradient",
      animated: textured,
    });
    return (
      <a
        ref={ref}
        data-press="off"
        className={buttonVariants({ variant, size, className })}
        {...props}
        {...(textured ? handlers : {})}
      >
        {textured ? surface : null}
        <span className="relative inline-flex items-center gap-2">
          {children}
        </span>
      </a>
    );
  },
);
ButtonLink.displayName = "ButtonLink";
