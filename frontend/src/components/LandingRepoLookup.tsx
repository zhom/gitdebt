import { useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight } from "lucide-react";

import { EYEBROW } from "@/components/style-tokens";
import { Button } from "@/components/ui/button";
import { DURATION, EASE_OUT, REDUCED_MOTION_DURATION } from "@/lib/motion";

const EXAMPLES = ["facebook/react", "vercel/next.js", "zhom/donutbrowser"];
const REPO_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;

export function LandingRepoLookup() {
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const reduceMotion = useReducedMotion();

  function openReport(value: string) {
    const match = value.trim().match(REPO_RE);
    if (!match) {
      setError("Enter a repository as owner/repo.");
      return;
    }
    window.location.assign(
      `/${encodeURIComponent(match[1].toLowerCase())}/${encodeURIComponent(match[2].toLowerCase())}`,
    );
  }

  function submit(event: React.SubmitEvent<HTMLFormElement>) {
    event.preventDefault();
    setError(null);
    openReport(input);
  }

  function chooseExample(example: string) {
    setInput(example);
    setError(null);
    inputRef.current?.focus();
  }

  return (
    <div className="w-full">
      <form
        onSubmit={submit}
        className="overflow-hidden rounded-lg border border-border/60 bg-background/40 transition-[border-color] duration-150 focus-within:border-accent/70"
      >
        <div className="flex min-h-14 items-stretch">
          <label
            htmlFor="landing-repo"
            className="flex shrink-0 items-center pl-4 font-mono text-[13px] text-muted-foreground select-none"
          >
            github.com/
          </label>
          <input
            ref={inputRef}
            id="landing-repo"
            name="repo"
            value={input}
            onChange={(event) => setInput(event.target.value)}
            placeholder="owner/repo"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
            aria-label="GitHub repository, as owner/repo"
            aria-invalid={error ? true : undefined}
            aria-describedby={error ? "landing-repo-error" : undefined}
            className="min-w-0 flex-1 bg-transparent px-1 py-3 font-mono text-[13px] text-foreground outline-none placeholder:text-muted-foreground/65"
          />
          <Button
            type="submit"
            variant="primary"
            className="group m-2 shrink-0 self-center"
          >
            <span className="hidden sm:inline">Analyze repository</span>
            <span className="sm:hidden">Analyze</span>
            <ArrowRight
              className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
              strokeWidth={1.75}
              aria-hidden="true"
            />
          </Button>
        </div>
      </form>

      <AnimatePresence initial={false}>
        {error && (
          <motion.p
            id="landing-repo-error"
            key={error}
            initial={{ opacity: 0, y: reduceMotion ? 0 : -4 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{
              duration: reduceMotion
                ? REDUCED_MOTION_DURATION
                : DURATION.feedback,
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
        <p className={EYEBROW}>Try</p>
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            data-press="off"
            onClick={() => chooseExample(example)}
            className="dither-chip outline-none focus-visible:ring-2 focus-visible:ring-accent/30"
          >
            {example}
          </button>
        ))}
      </div>
    </div>
  );
}
