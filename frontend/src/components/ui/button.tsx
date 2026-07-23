import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";

import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "dither-control inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-md font-mono text-sm font-medium outline-none disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 [&_svg]:shrink-0 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring",
  {
    variants: {
      variant: {
        default: "dither-primary text-primary-foreground",
        destructive:
          "text-destructive focus-visible:outline-destructive",
        outline: "border border-input text-foreground",
        secondary: "text-secondary-foreground",
        ghost: "text-muted-foreground hover:text-foreground",
        link: "text-foreground underline decoration-primary decoration-2 underline-offset-4 hover:decoration-primary/60",
      },
      size: {
        default: "h-11 px-3.5 py-2 has-[>svg]:px-3 sm:h-9",
        sm: "h-10 rounded-md px-3 has-[>svg]:px-2.5 sm:h-8",
        lg: "h-12 rounded-md px-4 text-base has-[>svg]:pr-4 has-[>svg]:pl-3 sm:h-11 sm:text-[0.9375rem]",
        icon: "size-12 sm:size-9",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(
  ({ className, variant, size, type = "button", ...props }, ref) => (
    <button
      ref={ref}
      type={type}
      data-press={variant === "link" ? "off" : undefined}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  ),
);
Button.displayName = "Button";

export { buttonVariants };
