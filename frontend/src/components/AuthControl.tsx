"use client";

import { useEffect, useId, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ChevronDown, LogOut, UserRound } from "lucide-react";

import { ButtonLink } from "@/components/ButtonLink";
import { Button } from "@/components/ui/button";
import { CONTROL_FOCUS, POPOVER } from "@/components/ui/dither-surface";
import { SPRING } from "@/lib/motion";
import { cn } from "@/lib/utils";

type User = {
  id: number;
  login: string;
  name: string | null;
  avatar_url: string | null;
  email: string | null;
};

type Props = {
  apiBase: string;
  returnTo?: string;
};

/** Menu row: mono microtype on a 36px target. */
const MENU_ITEM = cn(
  "flex min-h-9 w-full items-center gap-2.5 rounded-md px-2.5 text-left font-mono text-[12px] text-muted-foreground transition-colors duration-150 hover:bg-card/60 hover:text-foreground",
  CONTROL_FOCUS,
);

export function AuthControl({ apiBase, returnTo = "/" }: Props) {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const reduceMotion = useReducedMotion();
  const menuId = `${useId().replaceAll(":", "")}-account-menu`;

  useEffect(() => {
    const controller = new AbortController();
    void fetch(`${apiBase}/api/me`, {
      credentials: "include",
      headers: { accept: "application/json" },
      signal: controller.signal,
    })
      .then(async (response) => {
        if (!response.ok) return null;
        return (await response.json()) as User;
      })
      .then(setUser)
      .catch(() => undefined)
      .finally(() => setLoading(false));

    return () => controller.abort();
  }, [apiBase]);

  useEffect(() => {
    if (!open) return;
    function closeOnOutsidePress(event: PointerEvent) {
      if (!rootRef.current?.contains(event.target as Node)) setOpen(false);
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key !== "Escape") return;
      setOpen(false);
      triggerRef.current?.focus();
    }
    document.addEventListener("pointerdown", closeOnOutsidePress);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePress);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [open]);

  const currentPath =
    typeof window === "undefined"
      ? returnTo
      : `${window.location.pathname}${window.location.search}`;
  const loginHref = `${apiBase}/auth/github/start?return_to=${encodeURIComponent(currentPath)}`;

  async function signOut() {
    try {
      await fetch(`${apiBase}/auth/logout`, {
        method: "POST",
        credentials: "include",
        redirect: "manual",
      });
    } finally {
      setUser(null);
      setOpen(false);
    }
  }

  function focusItem(step: number) {
    const items =
      menuRef.current?.querySelectorAll<HTMLElement>('[role="menuitem"]');
    if (!items || items.length === 0) return;
    const index = [...items].findIndex((item) => item === document.activeElement);
    const next =
      index < 0
        ? step > 0
          ? 0
          : items.length - 1
        : (index + step + items.length) % items.length;
    items[next].focus();
  }

  if (!user) {
    return (
      <ButtonLink
        href={loginHref}
        aria-label="Login with GitHub"
        aria-busy={loading}
        variant="primary"
      >
        <span>Login</span>
        <GitHubMark className="size-4" />
      </ButtonLink>
    );
  }

  return (
    <div ref={rootRef} className="relative">
      <Button
        ref={triggerRef}
        variant="soft"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => setOpen((value) => !value)}
        onKeyDown={(event) => {
          if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
          event.preventDefault();
          const step = event.key === "ArrowUp" ? -1 : 1;
          setOpen(true);
          requestAnimationFrame(() => focusItem(step));
        }}
      >
        {user.avatar_url ? (
          <img
            src={user.avatar_url}
            alt=""
            width={24}
            height={24}
            className="size-6 rounded-full [image-rendering:auto]"
          />
        ) : (
          <UserRound className="size-5" strokeWidth={1.8} aria-hidden="true" />
        )}
        <span className="hidden max-w-28 truncate sm:inline">{user.login}</span>
        <motion.span
          className="hidden sm:inline-flex"
          initial={false}
          animate={{ rotate: open ? 180 : 0 }}
          transition={reduceMotion ? { duration: 0 } : SPRING.snappy}
          aria-hidden="true"
        >
          <ChevronDown
            className="size-3.5 text-muted-foreground"
            strokeWidth={2}
          />
        </motion.span>
        <span className="sr-only">{`Open account menu for ${user.login}`}</span>
      </Button>

      <AnimatePresence>
        {open && (
          <motion.div
            ref={menuRef}
            id={menuId}
            role="menu"
            aria-label={`Account menu for ${user.login}`}
            // The panel grows out of the trigger corner it hangs from.
            style={{ transformOrigin: "top right" }}
            initial={{
              opacity: 0,
              scale: reduceMotion ? 1 : 0.92,
              filter: reduceMotion ? "blur(0px)" : "blur(7px)",
            }}
            animate={{ opacity: 1, scale: 1, filter: "blur(0px)" }}
            exit={{
              opacity: 0,
              scale: reduceMotion ? 1 : 0.96,
              filter: reduceMotion ? "blur(0px)" : "blur(5px)",
            }}
            transition={reduceMotion ? { duration: 0.12 } : SPRING.snappy}
            onKeyDown={(event) => {
              if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
              event.preventDefault();
              focusItem(event.key === "ArrowUp" ? -1 : 1);
            }}
            className={cn(
              POPOVER,
              "absolute right-0 z-50 mt-2 w-72 p-2 text-popover-foreground",
            )}
          >
            <div className="px-2.5 py-2">
              <p className="truncate text-[13px]">{user.name || user.login}</p>
              <p className="truncate font-mono text-[11px] text-muted-foreground">
                @{user.login}
              </p>
            </div>
            <div className="my-1.5 h-px bg-border/40" aria-hidden="true" />
            <a
              role="menuitem"
              href={`/profile?login=${encodeURIComponent(user.login.toLowerCase())}`}
              className={MENU_ITEM}
            >
              <UserRound
                className="size-4 shrink-0"
                strokeWidth={1.8}
                aria-hidden="true"
              />
              Your profile report
            </a>
            <button
              role="menuitem"
              type="button"
              data-press="off"
              onClick={signOut}
              className={MENU_ITEM}
            >
              <LogOut
                className="size-4 shrink-0"
                strokeWidth={1.8}
                aria-hidden="true"
              />
              Sign out
            </button>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}

function GitHubMark({ className }: { className?: string }) {
  return (
    <svg
      viewBox="0 0 16 16"
      className={className}
      fill="currentColor"
      aria-hidden="true"
    >
      <path d="M8 0C3.58 0 0 3.64 0 8.13c0 3.59 2.29 6.63 5.47 7.7.4.08.55-.17.55-.39 0-.19-.01-.83-.01-1.51-2.01.38-2.53-.5-2.69-.96-.09-.23-.48-.96-.82-1.15-.28-.15-.68-.53-.01-.54.63-.01 1.08.59 1.23.83.72 1.23 1.87.88 2.33.67.07-.53.28-.88.51-1.08-1.78-.21-3.64-.91-3.64-4.02 0-.89.31-1.62.82-2.19-.08-.21-.36-1.04.08-2.16 0 0 .67-.22 2.2.84A7.4 7.4 0 0 1 8 3.9c.68 0 1.36.09 2 .27 1.53-1.06 2.2-.84 2.2-.84.44 1.12.16 1.95.08 2.16.51.57.82 1.3.82 2.19 0 3.12-1.87 3.81-3.65 4.02.29.25.54.74.54 1.5 0 1.08-.01 1.95-.01 2.22 0 .22.15.47.55.39A8.12 8.12 0 0 0 16 8.13C16 3.64 12.42 0 8 0Z" />
    </svg>
  );
}
