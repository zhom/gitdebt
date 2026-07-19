import { useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ArrowRight } from "lucide-react";

import {
  DURATION,
  EASE_OUT,
  REDUCED_MOTION_DURATION,
} from "@/lib/motion";

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
    const query = new URLSearchParams({ repo: `${match[1]}/${match[2]}` });
    window.location.assign(`/report?${query.toString()}`);
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
      <form onSubmit={submit} className="border-y border-black">
        <div className="flex min-h-14 items-stretch sm:min-h-16">
          <label
            htmlFor="landing-repo"
            className="flex shrink-0 items-center pl-4 font-mono text-sm text-zinc-500 sm:pl-5"
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
            className="min-w-0 flex-1 bg-white px-1 py-3 font-mono text-base text-black outline-none placeholder:text-zinc-400 focus-visible:bg-zinc-50 sm:text-sm"
          />
          <button
            type="submit"
            className="group inline-flex min-h-14 shrink-0 items-center justify-center gap-2 bg-black px-4 text-sm font-medium text-white outline-none hover:bg-zinc-800 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-black sm:min-h-16 sm:px-6"
          >
            <span className="hidden sm:inline">Analyze repository</span>
            <span className="sm:hidden">Analyze</span>
            <ArrowRight
              className="size-4 transition-transform duration-150 group-hover:translate-x-0.5 motion-reduce:transition-none"
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
            className="mt-2 text-sm text-black"
            role="alert"
          >
            {error}
          </motion.p>
        )}
      </AnimatePresence>

      <div className="mt-4 flex flex-wrap items-center gap-x-4 gap-y-2">
        <p className="font-mono text-xs tracking-wide text-zinc-500 uppercase">
          Try
        </p>
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            onClick={() => chooseExample(example)}
            className="min-h-11 border-b border-zinc-300 font-mono text-sm text-zinc-700 outline-none hover:border-black hover:text-black focus-visible:outline-2 focus-visible:outline-offset-4 focus-visible:outline-black sm:min-h-0"
          >
            {example}
          </button>
        ))}
      </div>
    </div>
  );
}
