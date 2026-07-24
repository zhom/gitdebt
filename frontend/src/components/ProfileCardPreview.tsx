import { useEffect, useState } from "react";
import { motion, useReducedMotion } from "motion/react";

import { useDitherSurface } from "@/components/ui/dither-surface";
import { INK } from "@/lib/dither";
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
  const { surface, handlers } = useDitherSurface({
    fill: INK,
    variant: "gradient",
    edge: 0.5,
    alpha: 0.2,
    pulse: true,
  });

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
      href={`/${login}`}
      className="dither-fallback relative isolate block max-w-full overflow-hidden rounded-lg p-1.5 outline-none focus-visible:ring-2 focus-visible:ring-accent/30 focus-visible:ring-offset-2 focus-visible:ring-offset-background"
      whileHover={reduceMotion ? undefined : { y: -2, scale: 1.006 }}
      whileTap={reduceMotion ? undefined : { scale: 0.992 }}
      transition={SPRING.snappy}
      {...handlers}
    >
      {surface}
      <img
        src={`${apiBase}/api/users/${login}/card.svg?theme=${theme}&animate=1&v=${revision}&render=${MEDIA_RENDER_REVISION}`}
        alt={`gitdebt profile statistics for ${login}`}
        loading="lazy"
        decoding="async"
        className="relative block h-auto max-w-full"
      />
    </motion.a>
  );
}
