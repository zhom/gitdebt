import { useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight } from "lucide-react";

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
      <form onSubmit={submit} className="border-y border-foreground">
        <div className="flex min-h-14 items-stretch sm:min-h-16">
          <label
            htmlFor="landing-repo"
            className="flex shrink-0 items-center pl-4 font-mono text-sm text-muted-foreground sm:pl-5"
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
            className="min-w-0 flex-1 bg-background px-1 py-3 font-mono text-base text-foreground outline-none placeholder:text-muted-foreground/65 focus-visible:bg-muted sm:text-sm"
          />
          <button
            type="submit"
            className="dither-primary group inline-flex min-h-14 shrink-0 items-center justify-center gap-2 px-4 text-sm font-medium outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-ring sm:min-h-16 sm:px-6"
          >
            <span className="hidden sm:inline">Analyze repository</span>
            <span className="sm:hidden">Analyze</span>
            <ArrowRight
              className="dither-arrow size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
              strokeWidth={1.75}
              aria-hidden="true"
            />
          </button>
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
            className="mt-2 text-sm text-foreground"
            role="alert"
          >
            {error}
          </motion.p>
        )}
      </AnimatePresence>

      <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
        <p className="font-mono text-xs tracking-wide text-muted-foreground uppercase">
          Try
        </p>
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            onClick={() => chooseExample(example)}
            className="dither-control min-h-11 px-2 font-mono text-sm text-muted-foreground outline-none hover:text-foreground focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-ring sm:min-h-9"
          >
            {example}
          </button>
        ))}
      </div>
    </div>
  );
}
