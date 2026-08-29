import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

/**
 * Four actions, because a product this size has four kinds of action.
 *
 * What was here before had nine variants — default, primary, accent, soft,
 * outline, secondary, ghost, destructive, link — of which `outline` and
 * `secondary` were byte-identical and three more differed only in canvas
 * texture. Nine names rendering four treatments is not a system, it is drift.
 *
 * The hierarchy is carried by ink and ground, never by fill-versus-outline:
 *
 *   primary  the one action on a page. Drafting red, cut corner, the drawing's
 *            own lettering. There is at most one per surface.
 *   quiet    a working control — copy, embed, toggle a range. It has a drawn
 *            edge and takes paper under the pointer. It is NOT the outlined
 *            half of a filled/outlined pair; if a `primary` is on the same row,
 *            the other action is a `link`.
 *   link     a text action. It carries the leader arrow and it leaves.
 *   danger   the destructive action. Red ink on paper, never a red fill, so it
 *            can never be mistaken for the primary.
 *
 * Nothing here lifts, scales, glows, or grows an underline on hover. A control
 * changes state by changing ink and ground, in one frame budget.
 */
// `group` is load-bearing, not decoration: `ButtonLink` drives its leader arrow
// with `group-hover:`, and without a group established on the control itself
// that gesture never fires — the one authored motion an action has, dead at
// every call site.
const button = cva(
  "group relative inline-flex select-none items-center justify-center gap-2 whitespace-nowrap font-draft tracking-[0.04em] uppercase outline-none transition-[background-color,color,border-color] duration-[--duration-ui] disabled:pointer-events-none disabled:opacity-40 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      size: {
        sm: "min-h-9 px-3 text-[0.75rem]",
        default: "min-h-11 px-4 text-[0.8125rem]",
        lg: "min-h-12 px-5 text-[0.9375rem]",
        icon: "size-11 px-0",
      },
      variant: {
        primary:
          "cut [--cut:10px] [--pad-x:1rem] [--pad-y:0.5rem] bg-signal text-signal-ink hover:bg-[oklch(0.48_0.19_29)]",
        quiet:
          "border border-rule-strong bg-transparent text-ink hover:bg-paper hover:border-ink-3",
        link: "min-h-0 px-0 text-ink-2 normal-case tracking-normal font-sans hover:text-signal",
        danger:
          "border border-signal/40 bg-transparent text-signal hover:bg-signal-wash",
      },
    },
    defaultVariants: { variant: "quiet", size: "default" },
  },
);

/**
 * Merged so `.astro` call sites, which cannot run `cn()` on the result, still
 * get deterministic overrides instead of relying on stylesheet order.
 */
const buttonVariants = (props?: Parameters<typeof button>[0]) => cn(button(props));

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof button> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, type = "button", children, ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      className={buttonVariants({ variant, size, className })}
      {...props}
    >
      {children}
    </button>
  ),
);
Button.displayName = "Button";

export { buttonVariants };
