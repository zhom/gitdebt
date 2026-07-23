import { useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight } from "lucide-react";

import { EYEBROW } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION } from "@/lib/motion";

const EXAMPLES = ["facebook/react", "vercel/next.js", "zhom/donutbrowser"];

export function RepoLookup() {
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const reduceMotion = useReducedMotion();

  function go(value: string) {
    const match = value.trim().match(/^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/);
    if (!match) {
      setError("Enter a repository as owner/repo.");
      return;
    }
    const [, owner, repo] = match;
    window.location.assign(
      `/${encodeURIComponent(owner.toLowerCase())}/${encodeURIComponent(repo.toLowerCase())}`,
    );
  }

  function onSubmit(e: React.SubmitEvent<HTMLFormElement>) {
    e.preventDefault();
    setError(null);
    go(input);
  }

  function pick(example: string) {
    setError(null);
    setInput(example);
    inputRef.current?.focus();
  }

  return (
    <div className="mx-auto w-full max-w-xl">
      <form onSubmit={onSubmit} className="flex flex-col gap-3 sm:flex-row">
        <div
          className="flex min-h-10 flex-1 items-center rounded-md border border-border/60 bg-background/60 font-mono text-[13px] transition-[border-color] duration-150 hover:border-foreground/25 focus-within:border-accent/70"
          data-invalid={error ? "" : undefined}
        >
          <label
            htmlFor="repo-lookup"
            className="pl-3 text-muted-foreground select-none"
          >
            github.com/
          </label>
          <input
            ref={inputRef}
            id="repo-lookup"
            name="repo"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            placeholder="owner/repo"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            aria-label="GitHub repository, as owner/repo"
            aria-invalid={error ? true : undefined}
            className="w-full flex-1 bg-transparent py-2 pr-3 pl-1 text-foreground placeholder:text-muted-foreground/50 outline-none"
          />
        </div>
        <Button type="submit" size="lg" className="w-full sm:w-auto">
          View history
          <ArrowRight />
        </Button>
      </form>

      <AnimatePresence initial={false}>
        {error && (
          <motion.p
            key={error}
            initial={{
              opacity: 0,
              y: reduceMotion ? 0 : -4,
            }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, transition: { duration: 0.12 } }}
            transition={{
              duration: reduceMotion ? REDUCED_MOTION_DURATION : DURATION.enter,
              ease: EASE_OUT,
            }}
            className="mt-2 text-[11px] text-[var(--swatch-red)]"
            role="alert"
          >
            {error}
          </motion.p>
        )}
      </AnimatePresence>

      <div className="mt-4 flex flex-wrap items-center gap-2">
        <span className={EYEBROW}>Try</span>
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            data-press="off"
            onClick={() => pick(example)}
            className="dither-chip outline-none focus-visible:ring-2 focus-visible:ring-accent/30"
          >
            {example}
          </button>
        ))}
      </div>
    </div>
  );
}
