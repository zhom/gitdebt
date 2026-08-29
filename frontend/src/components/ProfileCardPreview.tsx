import { useEffect, useState } from "react";

import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { useRenderedTheme } from "@/lib/rendered-theme";

/**
 * The rendered profile card, linked to the profile it measures.
 *
 * It polls only while gitdebt still has repositories to read for this account,
 * and each answer changes the `v=` revision so the browser fetches the newer
 * card. That is real product behaviour and it stays.
 *
 * What went is the chrome: the card no longer lifts off the page under the
 * pointer, no longer scales when pressed, and no longer sits on a textured
 * pad. It is a drawing pinned to the sheet — the ground steps to paper and the
 * frame takes ink, and nothing moves.
 *
 * The image is a plain `<img>` with a real `src` in the served markup, so the
 * card is present before any script runs; the poll only ever replaces it with
 * a newer render of the same thing.
 */

type ProfileProgress = {
  total_stars?: number;
  repos_included?: number;
  repos_pending?: number;
  repos_analyzed?: number;
  repos_analyzing?: number;
};

const POLL_MS = 8_000;

export function ProfileCardPreview({
  apiBase,
  login,
  initialRevision = "initial",
  warm = true,
}: {
  apiBase: string;
  login: string;
  initialRevision?: string;
  warm?: boolean;
}) {
  const [revision, setRevision] = useState(initialRevision);
  const theme = useRenderedTheme();

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let warmAttempted = false;

    async function refresh() {
      try {
        let response: Response | null = null;
        if (warm && !warmAttempted) {
          warmAttempted = true;
          const warmed = await fetch(`${apiBase}/api/users/${login}/warm`, {
            method: "POST",
            credentials: "include",
            cache: "no-store",
            headers: { accept: "application/json" },
          });
          if (warmed.ok) response = warmed;
        }
        response ??= await fetch(`${apiBase}/api/users/${login}/analyze`, {
          cache: "no-store",
          credentials: "omit",
          headers: { accept: "application/json" },
        });
        if (!response.ok) return;
        const data = (await response.json()) as ProfileProgress;
        if (cancelled) return;
        setRevision(
          [
            data.total_stars ?? 0,
            data.repos_included ?? 0,
            data.repos_pending ?? 0,
            data.repos_analyzed ?? 0,
            data.repos_analyzing ?? 0,
          ].join("-"),
        );
        if ((data.repos_pending ?? 0) > 0 || (data.repos_analyzing ?? 0) > 0) {
          timer = setTimeout(refresh, POLL_MS);
        }
      } catch {
        // The already-rendered card remains useful if a refresh is interrupted.
      }
    }

    void refresh();
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
    };
  }, [apiBase, login, warm]);

  return (
    <a
      href={`/${login}`}
      className="block max-w-full border border-rule-strong bg-paper p-1.5 outline-none transition-colors duration-[--duration-ui] hover:border-ink-3 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
    >
      <img
        src={`${apiBase}/api/users/${login}/card.svg?theme=${theme}&animate=1&v=${revision}&render=${MEDIA_RENDER_REVISION}`}
        alt={`gitdebt profile statistics for ${login}`}
        loading="lazy"
        decoding="async"
        className="block h-auto max-w-full"
      />
    </a>
  );
}
