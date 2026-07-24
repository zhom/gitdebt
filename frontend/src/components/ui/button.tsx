import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { BRAND, SWATCH, type RGB } from "@/lib/dither";
import { cn } from "@/lib/utils";
import { useDitherSurface } from "@/components/ui/dither-surface";

/**
 * Hierarchy comes from the presence or absence of texture: only `primary` and
 * `accent` carry a dithered canvas. Everything else is flat, with border and
 * text doing the work.
 */
const button = cva(
  "relative isolate inline-flex min-h-10 items-center justify-center gap-2 overflow-hidden rounded-md px-4 py-2 font-mono text-xs whitespace-nowrap outline-none transition-[opacity,scale] active:scale-[0.96] motion-reduce:transition-none focus-visible:ring-2 focus-visible:ring-accent/30 focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:pointer-events-none disabled:opacity-40 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 [&_svg]:shrink-0",
  {
    // `size` is declared first so variant classes win the twMerge pass.
    variants: {
      size: {
        sm: "min-h-9 px-3 text-[11px]",
        default: "min-h-10 px-4 text-xs",
        lg: "min-h-11 px-5 text-[13px]",
        icon: "size-10 px-0",
      },
      variant: {
        default: "text-foreground",
        primary: "text-foreground",
        accent: "text-foreground",
        soft: "border border-border/60 text-foreground",
        outline:
          "border border-border/60 text-foreground transition-[border-color,background-color] duration-150 hover:border-foreground/25 hover:bg-card/60",
        secondary:
          "border border-border/60 text-foreground transition-[border-color,background-color] duration-150 hover:border-foreground/25 hover:bg-card/60",
        ghost:
          "text-muted-foreground transition-colors duration-150 hover:bg-card/60 hover:text-foreground",
        destructive:
          "border border-[color-mix(in_oklab,var(--swatch-red)_35%,transparent)] text-[var(--swatch-red)] transition-colors duration-150 hover:bg-[color-mix(in_oklab,var(--swatch-red)_10%,transparent)]",
        link: "min-h-0 px-0 text-foreground/80 underline decoration-border underline-offset-4 transition-colors duration-150 hover:decoration-foreground/60 active:scale-100",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

/**
 * Merged so that `.astro` call sites, which cannot run `cn()` on the result,
 * still get deterministic overrides instead of relying on stylesheet order.
 */
const buttonVariants = (props?: Parameters<typeof button>[0]) =>
  cn(button(props));

type ButtonVariant = NonNullable<VariantProps<typeof button>["variant"]>;

const CANVAS_FILL: Partial<Record<ButtonVariant, RGB>> = {
  default: BRAND,
  primary: BRAND,
  accent: SWATCH.blue,
  soft: BRAND,
};

/**
 * `soft` keeps the texture but at a fraction of the strength, so a copy or
 * embed action reads as dithered without competing with the page's one primary.
 */
const CANVAS_ALPHA: Partial<Record<ButtonVariant, number>> = {
  soft: 0.42,
};

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof button> {
  /**
   * Adds the cursor-positioned one-shot pulse. Textured variants opt in by
   * default; quiet variants only pulse when an occasional action asks for it.
   */
  pulse?: boolean;
}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  (
    {
      className,
      variant,
      size,
      type = "button",
      disabled,
      pulse,
      children,
      ...props
    },
    ref,
  ) => {
    const fill = CANVAS_FILL[variant ?? "default"];
    const textured = fill !== undefined && !disabled;
    const pulseEnabled = !disabled && (pulse ?? textured);
    const surfaceEnabled = textured || pulseEnabled;
    const { surface, handlers } = useDitherSurface({
      fill: fill ?? SWATCH.blue,
      variant: "gradient",
      animated: surfaceEnabled,
      alpha: textured ? CANVAS_ALPHA[variant ?? "default"] : 0,
      pulse: pulseEnabled,
    });
    return (
      <button
        ref={ref}
        type={type}
        disabled={disabled}
        // Press feedback is owned here (`active:scale-[0.96]` plus an intensity
        // jump), so the global fallback press transform stays out of the way.
        data-press="off"
        className={buttonVariants({ variant, size, className })}
        {...props}
        {...(surfaceEnabled ? handlers : {})}
      >
        {surfaceEnabled ? surface : null}
        <span className="relative inline-flex items-center gap-2">
          {children}
        </span>
      </button>
    );
  },
);
Button.displayName = "Button";

export { buttonVariants };
