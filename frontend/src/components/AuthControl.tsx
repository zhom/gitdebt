import { useEffect, useRef, useState } from "react";
import { ChevronDown, LogOut, UserRound } from "lucide-react";

type User = {
  id: number;
  login: string;
  name: string | null;
  avatar_url: string | null;
  email: string | null;
};

type Props = {
  apiBase: string;
};

export function AuthControl({ apiBase }: Props) {
  const [user, setUser] = useState<User | null>(null);
  const [loading, setLoading] = useState(true);
  const detailsRef = useRef<HTMLDetailsElement>(null);

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
    function closeOnOutsidePress(event: PointerEvent) {
      const details = detailsRef.current;
      if (details?.open && !details.contains(event.target as Node)) {
        details.open = false;
      }
    }

    function closeOnEscape(event: KeyboardEvent) {
      const details = detailsRef.current;
      if (details?.open && event.key === "Escape") {
        details.open = false;
        details.querySelector<HTMLElement>("summary")?.focus();
      }
    }

    document.addEventListener("pointerdown", closeOnOutsidePress);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOnOutsidePress);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, []);

  if (loading) {
    return (
      <div
        className="h-10 w-10 rounded-full bg-muted motion-safe:animate-pulse sm:w-24 sm:rounded-md"
        aria-label="Checking sign-in status"
      />
    );
  }

  const currentPath =
    typeof window === "undefined"
      ? "/"
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
      if (detailsRef.current) detailsRef.current.open = false;
    }
  }

  return (
    <details ref={detailsRef} className="relative">
      <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium hover:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden">
        {user?.avatar_url ? (
          <img
            src={user.avatar_url}
            alt=""
            width={24}
            height={24}
            className="size-6 rounded-full"
          />
        ) : (
          <UserRound className="size-5" strokeWidth={1.8} aria-hidden="true" />
        )}
        <span className="hidden max-w-28 truncate sm:inline">
          {user ? user.login : "Sign in"}
        </span>
        <ChevronDown
          className="hidden size-3.5 text-muted-foreground sm:block"
          strokeWidth={2}
          aria-hidden="true"
        />
        <span className="sr-only">
          {user ? `Open account menu for ${user.login}` : "Open sign-in menu"}
        </span>
      </summary>

      <div className="absolute right-0 z-50 mt-2 w-72 rounded-lg border border-border bg-popover p-3 text-popover-foreground">
        {user ? (
          <>
            <div className="px-2 py-2">
              <p className="truncate text-sm font-medium">
                {user.name || user.login}
              </p>
              <p className="truncate font-mono text-xs text-muted-foreground">
                @{user.login}
              </p>
            </div>
            <div className="my-2 h-px bg-border" aria-hidden="true" />
            <a
              href={`/profile?login=${encodeURIComponent(user.login.toLowerCase())}`}
              className="flex min-h-11 items-center gap-2 rounded-md px-2.5 py-2 text-sm hover:bg-accent focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
            >
              <UserRound className="size-4" strokeWidth={1.8} aria-hidden="true" />
              Your profile report
            </a>
            <button
              type="button"
              onClick={signOut}
              className="flex min-h-11 w-full items-center gap-2 rounded-md px-2.5 py-2 text-left text-sm text-muted-foreground hover:bg-accent hover:text-foreground focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-ring"
            >
              <LogOut className="size-4" strokeWidth={1.8} aria-hidden="true" />
              Sign out
            </button>
          </>
        ) : (
          <div className="space-y-3 p-2">
            <div className="space-y-1">
              <p className="text-sm font-medium">Optional GitHub sign-in</p>
              <p className="text-sm leading-relaxed text-pretty text-muted-foreground">
                Sign in for a one-click profile report. Public repository
                analysis stays available without an account.
              </p>
            </div>
            <a
              href={loginHref}
              className="inline-flex min-h-11 w-full items-center justify-center gap-2 rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground hover:brightness-95 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring"
            >
              <UserRound className="size-4" strokeWidth={1.8} aria-hidden="true" />
              Continue with GitHub
            </a>
            <p className="text-xs leading-relaxed text-muted-foreground">
              Requests only your public GitHub identity. It does not unlock
              private repository analysis.
            </p>
          </div>
        )}
      </div>
    </details>
  );
}
