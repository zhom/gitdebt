import { useRef, useState } from "react";

import { CAPTION } from "@/components/style-tokens";
import { Terminator } from "@/components/ui/marks";
import { cn } from "@/lib/utils";

/**
 * The drawing's input field.
 *
 * What was here was the single most-shipped component on the internet: a
 * pill-shaped input with a pill-shaped filled button welded to its right edge
 * and a horizontal arrow inside it. That row is a named template, and no
 * amount of recolouring rescues it.
 *
 * This is a field on a drawing instead. The rule under the entry is a real
 * dimension line — it spans the whole entry from its origin to its terminator
 * — and the terminator at its right end is the submit control, so the mark
 * that says "this measurement ends here" is the same mark that says "take the
 * measurement". Nothing about it is a pill, and there is no second button.
 *
 * The field label lives on the page above (`index.astro` letters it as the
 * drawing does), so the input carries its accessible name on `aria-label` and
 * the visible `github.com/` prefix stays what it actually is: the fixed part
 * of the value, not a label.
 */

const EXAMPLES = ["facebook/react", "vercel/next.js", "zhom/donutbrowser"];
const REPO_RE = /^([A-Za-z0-9._-]+)\/([A-Za-z0-9._-]+)$/;

export function LandingRepoLookup() {
  const [input, setInput] = useState("");
  const [error, setError] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  function openReport(value: string) {
    const match = value.trim().match(REPO_RE);
    if (!match) {
      setError("Enter a repository as owner/repo.");
      inputRef.current?.focus();
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
      <form onSubmit={submit} noValidate>
        {/*
          The entry and its dimension line. The rule is drawn at rest and only
          changes ink when the field is focused — it never grows, wipes or
          travels, because a line that animates its own length here would be
          measuring nothing while it did so.
        */}
        <div className="flex items-center border-b border-rule-strong transition-colors duration-[--duration-ui] focus-within:border-signal">
          <label
            htmlFor="landing-repo"
            className="shrink-0 cursor-text py-2.5 font-mono text-[0.8125rem] text-ink-3 select-none"
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
            autoComplete="off"
            spellCheck={false}
            enterKeyHint="go"
            aria-label="GitHub repository, as owner/repo"
            aria-invalid={error ? true : undefined}
            aria-describedby="landing-repo-error"
            className="min-w-0 flex-1 bg-transparent py-2.5 font-mono text-[0.8125rem] text-ink outline-none placeholder:text-ink-3"
          />
          <button
            type="submit"
            aria-label="Open this repository's report"
            title="Open this repository's report"
            className="grid size-11 shrink-0 place-items-center text-ink-2 outline-none transition-colors duration-[--duration-ui] hover:text-signal focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal"
          >
            <Terminator size={18} />
          </button>
        </div>
      </form>

      {/*
        The message keeps its line whether or not there is a message, so a
        rejected entry corrects the field instead of shoving the page down.
      */}
      <p
        id="landing-repo-error"
        role="alert"
        className="mt-2 min-h-[1.125rem] text-[0.75rem] leading-[1.5] text-signal"
      >
        {error}
      </p>

      <div
        role="group"
        aria-label="Example repositories"
        className="mt-1 flex flex-wrap items-center gap-x-5"
      >
        <span className={CAPTION}>Try</span>
        {EXAMPLES.map((example) => (
          <button
            key={example}
            type="button"
            onClick={() => chooseExample(example)}
            className={cn(
              "inline-flex min-h-11 items-center font-mono text-[0.75rem] text-ink-2 outline-none",
              "transition-colors duration-[--duration-ui] hover:text-signal",
              "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-signal",
            )}
          >
            {example}
          </button>
        ))}
      </div>
    </div>
  );
}
