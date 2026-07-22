import { useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight } from "lucide-react";

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
          className="flex flex-1 items-center rounded-md border border-input bg-card font-mono text-base focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-ring sm:text-sm"
          data-invalid={error ? "" : undefined}
        >
          <label
            htmlFor="repo-lookup"
            className="pl-3.5 text-muted-foreground select-none"
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
            className="w-full flex-1 bg-transparent py-2.5 pr-3.5 pl-1 text-foreground placeholder:text-muted-foreground/50 outline-none"
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
            className="mt-2 text-base text-destructive sm:text-sm"
            role="alert"
          >
            {error}
          </motion.p>
        )}
      </AnimatePresence>

      <div className="mt-4 flex flex-wrap items-center gap-2 text-sm">
        <span className="font-mono tracking-wide text-muted-foreground uppercase">
          Try
        </span>
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            onClick={() => pick(example)}
            className="min-h-11 rounded-md border border-border bg-card px-2.5 py-2 font-mono text-foreground/80 hover:bg-accent hover:text-accent-foreground active:bg-accent/70 sm:min-h-0 sm:py-1"
          >
            {example}
          </button>
        ))}
      </div>
    </div>
  );
}
