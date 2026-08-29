import { buttonVariants } from "@/components/ui/button";

type Props = {
  href: string;
};

/**
 * Sign in with GitHub.
 *
 * The GitHub mark is the real one, drawn bare on the surface: no tile, no chip,
 * no circle behind it. It takes the ink of the control it sits in, so it reads
 * as part of the lettering rather than as a logo pasted beside it.
 *
 * Nothing here lifts, scales, rotates or swaps its own label for a cuter one
 * under the pointer. It is a working control on a drawing, and it changes state
 * the way every other control on this site does: ground and ink, in one frame.
 */
export function GitHubLoginButton({ href }: Props) {
  return (
    <a
      href={href}
      aria-label="Sign in with GitHub"
      className={buttonVariants({ variant: "quiet", size: "default" })}
    >
      <svg
        viewBox="0 0 16 16"
        className="size-4"
        fill="currentColor"
        aria-hidden="true"
        focusable="false"
      >
        <path d="M8 0C3.58 0 0 3.64 0 8.13c0 3.59 2.29 6.63 5.47 7.7.4.08.55-.17.55-.39 0-.19-.01-.83-.01-1.51-2.01.38-2.53-.5-2.69-.96-.09-.23-.48-.96-.82-1.15-.28-.15-.68-.53-.01-.54.63-.01 1.08.59 1.23.83.72 1.23 1.87.88 2.33.67.07-.53.28-.88.51-1.08-1.78-.21-3.64-.91-3.64-4.02 0-.89.31-1.62.82-2.19-.08-.21-.36-1.04.08-2.16 0 0 .67-.22 2.2.84A7.4 7.4 0 0 1 8 3.9c.68 0 1.36.09 2 .27 1.53-1.06 2.2-.84 2.2-.84.44 1.12.16 1.95.08 2.16.51.57.82 1.3.82 2.19 0 3.12-1.87 3.81-3.65 4.02.29.25.54.74.54 1.5 0 1.08-.01 1.95-.01 2.22 0 .22.15.47.55.39A8.12 8.12 0 0 0 16 8.13C16 3.64 12.42 0 8 0Z" />
      </svg>
      <span>Sign in</span>
    </a>
  );
}
