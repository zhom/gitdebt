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
  returnTo?: string;
};

export function AuthControl({ apiBase, returnTo = "/" }: Props) {
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
      if (detailsRef.current) detailsRef.current.open = false;
    }
  }

  if (!user) {
    return (
      <a
        href={loginHref}
        aria-label="Login with GitHub"
        aria-busy={loading}
        className="inline-flex min-h-12 items-center justify-center gap-2 rounded-md bg-primary px-3.5 py-2 text-sm font-medium text-primary-foreground transition-colors duration-150 hover:bg-primary/90 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring active:bg-primary/85"
      >
        <span>Login</span>
        <GitHubMark className="size-4" />
      </a>
    );
  }

  return (
    <details ref={detailsRef} className="relative">
      <summary className="flex min-h-11 cursor-pointer list-none items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm font-medium hover:bg-accent focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring [&::-webkit-details-marker]:hidden">
        {user.avatar_url ? (
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
        <span className="hidden max-w-28 truncate sm:inline">{user.login}</span>
        <ChevronDown
          className="hidden size-3.5 text-muted-foreground sm:block"
          strokeWidth={2}
          aria-hidden="true"
        />
        <span className="sr-only">{`Open account menu for ${user.login}`}</span>
      </summary>

      <div className="absolute right-0 z-50 mt-2 w-72 rounded-lg border border-border bg-popover p-3 text-popover-foreground">
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
      </div>
    </details>
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
