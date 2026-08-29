import * as React from "react";

import { buttonVariants } from "@/components/ui/button";
import { Leader } from "@/components/ui/marks";

type ButtonVariants = Omit<
  NonNullable<Parameters<typeof buttonVariants>[0]>,
  "class" | "className"
>;

export type ButtonLinkProps = React.AnchorHTMLAttributes<HTMLAnchorElement> &
  ButtonVariants & {
    /**
     * Appends the leader arrow. On by default for `link`, because a text
     * action needs to read as one; off elsewhere, because a filled action is
     * already unmistakably an action and an arrow on it is decoration.
     */
    leader?: boolean;
  };

/**
 * An anchor wearing the action treatment. Navigation stays an `<a>`.
 *
 * The leader arrow travels on hover — the one authored gesture an action gets.
 * It moves up and to the right, along its own axis, which is the direction it
 * points; a button that jumps upward on hover is the template's reflex and
 * this one does not move at all.
 */
export const ButtonLink = React.forwardRef<HTMLAnchorElement, ButtonLinkProps>(
  ({ className, variant, size, leader, children, ...props }, ref) => {
    const showLeader = leader ?? variant === "link";
    return (
      <a
        ref={ref}
        className={buttonVariants({ variant, size, className })}
        {...props}
      >
        {children}
        {showLeader && (
          <Leader
            size={13}
            className="transition-transform duration-[--duration-ui] group-hover:-translate-y-px group-hover:translate-x-px motion-reduce:transition-none"
          />
        )}
      </a>
    );
  },
);
ButtonLink.displayName = "ButtonLink";
