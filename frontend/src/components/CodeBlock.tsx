import { useMemo, type ReactNode } from "react";

import { CopyButton } from "@/components/CopyButton";
import { DATUM } from "@/components/style-tokens";
import { cn } from "@/lib/utils";

/**
 * A snippet, on paper, inside a frame.
 *
 * The snippet is genuinely data, so it stays in the mono face — that is what
 * the mono face is for on this site, and the one place it is not a costume.
 * Everything around it is drawing: a frame rule encloses the block, a header
 * rule separates the file it belongs to from the bytes themselves, and the copy
 * action confirms by ink rather than by lifting off the page.
 *
 * Highlighting is graphite, not a rainbow. The sheet has three steps of ink and
 * exactly one saturated colour, and that colour is spent on measured values —
 * never on a punctuation mark. So structure reads through weight of ink, a URL
 * reads through its rule, and a comment reads through its slant. Three real
 * distinctions beat eight arbitrary hues.
 *
 * Highlighting happens in the browser rather than through a build-time grammar
 * engine: the product ships five short snippet dialects, and a full highlighter
 * would cost more bytes than every snippet on the site combined.
 */
export type CodeLanguage = "markdown" | "html" | "bash" | "json" | "text";

type TokenKind =
  | "plain"
  | "tag"
  | "attr"
  | "string"
  | "comment"
  | "punct"
  | "keyword"
  | "number"
  | "url";

export type Token = { text: string; kind: TokenKind };

/** Concrete classes so Tailwind emits them; the map is never built at runtime. */
export const TOKEN_CLASS: Record<TokenKind, string> = {
  plain: "text-ink-2",
  tag: "text-ink",
  attr: "text-ink-2",
  string: "text-ink-2",
  comment: "text-ink-3 italic",
  punct: "text-ink-3",
  keyword: "text-ink",
  number: "text-ink tabular-nums",
  url: "text-ink-2 underline decoration-rule-strong underline-offset-[3px]",
};

/**
 * Ordered alternation. The first group that matches decides the kind, and any
 * gap between matches is emitted as `plain`, so a rule can never swallow text.
 */
type Rule = { re: RegExp; kind: TokenKind };

const HTML_RULES: Rule[] = [
  { re: /<!--[\s\S]*?-->/y, kind: "comment" },
  { re: /<\/?[a-zA-Z][\w:.-]*/y, kind: "tag" },
  { re: /"[^"\n]*"|'[^'\n]*'/y, kind: "string" },
  { re: /[a-zA-Z_:][\w:.-]*(?=\s*=)/y, kind: "attr" },
  { re: /\/?>|[=]/y, kind: "punct" },
];

const MARKDOWN_RULES: Rule[] = [
  { re: /`[^`\n]*`/y, kind: "string" },
  { re: /^#{1,6}\s.*$/my, kind: "keyword" },
  { re: /https?:\/\/[^\s)<>"']+/y, kind: "url" },
  ...HTML_RULES,
  { re: /!?\[|\]\(|[)\]]/y, kind: "punct" },
];

const BASH_RULES: Rule[] = [
  { re: /#[^\n]*/y, kind: "comment" },
  { re: /"[^"\n]*"|'[^'\n]*'/y, kind: "string" },
  // Ahead of the flag rule so a hyphen inside a URL is not read as an option.
  { re: /https?:\/\/[^\s'"]+/y, kind: "url" },
  { re: /\b(?:curl|npx|npm|pnpm|git|cd|echo|sudo|export)\b/y, kind: "keyword" },
  { re: /-{1,2}[a-zA-Z][\w-]*/y, kind: "attr" },
  { re: /[|;&><]/y, kind: "punct" },
];

const JSON_RULES: Rule[] = [
  { re: /"(?:[^"\\\n]|\\.)*"(?=\s*:)/y, kind: "attr" },
  { re: /"(?:[^"\\\n]|\\.)*"/y, kind: "string" },
  { re: /\b(?:true|false|null)\b/y, kind: "keyword" },
  { re: /-?\d+(?:\.\d+)?(?:[eE][+-]?\d+)?/y, kind: "number" },
  { re: /[{}[\],:]/y, kind: "punct" },
];

const TEXT_RULES: Rule[] = [
  // Stops at the query so the parameters read as structure, not as one blob.
  { re: /https?:\/\/[^\s?#]+/y, kind: "url" },
  { re: /[?&#=]/y, kind: "punct" },
];

const RULES: Record<CodeLanguage, Rule[]> = {
  markdown: MARKDOWN_RULES,
  html: HTML_RULES,
  bash: BASH_RULES,
  json: JSON_RULES,
  text: TEXT_RULES,
};

export function tokenize(source: string, language: CodeLanguage): Token[] {
  const rules = RULES[language] ?? TEXT_RULES;
  const tokens: Token[] = [];
  let plain = "";
  let i = 0;
  const flush = () => {
    if (plain) tokens.push({ text: plain, kind: "plain" });
    plain = "";
  };
  while (i < source.length) {
    let matched = false;
    for (const rule of rules) {
      rule.re.lastIndex = i;
      const hit = rule.re.exec(source);
      if (!hit || hit.index !== i || hit[0].length === 0) continue;
      flush();
      tokens.push({ text: hit[0], kind: rule.kind });
      i += hit[0].length;
      matched = true;
      break;
    }
    if (matched) continue;
    plain += source[i];
    i += 1;
  }
  flush();
  return tokens;
}

export type CodeBlockProps = {
  code: string;
  language?: CodeLanguage;
  /** What the snippet IS, e.g. `README.md · facebook/react`. A filename and a
   *  slug are data, so this is lettered in the mono face and never uppercased —
   *  a repository slug is case-sensitive and a label may not rewrite it. */
  label?: ReactNode;
  copyLabel?: string;
  copyAriaLabel?: string;
  className?: string;
  /** Tailwind max-height utility applied to the scroll region. */
  maxHeightClass?: string;
};

export function CodeBlock({
  code,
  language = "text",
  label,
  copyLabel = "Copy",
  copyAriaLabel,
  className,
  maxHeightClass = "max-h-56",
}: CodeBlockProps) {
  const tokens = useMemo(() => tokenize(code, language), [code, language]);

  return (
    <div className={cn("border border-rule-strong bg-paper", className)}>
      {(label || copyLabel) && (
        <div className="flex min-h-11 items-center justify-between gap-4 border-b border-rule px-3">
          <span className={cn(DATUM, "min-w-0 truncate text-ink-3")}>
            {label}
          </span>
          <CopyButton
            value={code}
            idleLabel={copyLabel}
            ariaLabel={copyAriaLabel ?? copyLabel}
            variant="link"
            /* The row is the touch target: full height, and padded out to a
               real width while staying optically flush with the frame. */
            className="-mr-2 min-h-11 shrink-0 px-2"
          />
        </div>
      )}
      <pre
        className={cn(
          "overflow-auto px-3 py-3 font-mono text-[0.75rem] leading-[1.7] text-ink",
          maxHeightClass,
        )}
      >
        <code>
          {tokens.map((token, index) => (
            <span key={index} className={TOKEN_CLASS[token.kind]}>
              {token.text}
            </span>
          ))}
        </code>
      </pre>
    </div>
  );
}
