import { useEffect, useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import { MEDIA_RENDER_REVISION } from "@/lib/media";
import { SPRING } from "@/lib/motion";
import { useRenderedTheme } from "@/lib/rendered-theme";

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
  const reduceMotion = useReducedMotion();

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let warmAttempted = false;

    async function refresh() {
      try {
        let response: Response | null = null;
        if (warm && !warmAttempted) {
          warmAttempted = true;
          const warm = await fetch(`${apiBase}/api/users/${login}/warm`, {
            method: "POST",
            credentials: "include",
            cache: "no-store",
            headers: { accept: "application/json" },
          });
          if (warm.ok) response = warm;
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
    <motion.a
      href={`/u/${login}`}
      className="dither-badge-bed block max-w-full rounded-xl p-1.5 focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring"
      whileHover={reduceMotion ? undefined : { y: -2, scale: 1.006 }}
      whileTap={reduceMotion ? undefined : { scale: 0.992 }}
      transition={SPRING.snappy}
    >
      <img
        src={`${apiBase}/api/users/${login}/card.svg?theme=${theme}&animate=1&v=${revision}&render=${MEDIA_RENDER_REVISION}`}
        alt={`gitdebt profile statistics for ${login}`}
        loading="lazy"
        decoding="async"
        className="block h-auto max-w-full"
      />
    </motion.a>
  );
}
