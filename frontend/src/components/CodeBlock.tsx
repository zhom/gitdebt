"use client";

import { useMemo, type ReactNode } from "react";

import { CopyButton } from "@/components/CopyButton";
import { EYEBROW } from "@/components/style-tokens";
import { DitherSurface } from "@/components/ui/dither-surface";
import { BRAND } from "@/lib/dither";
import { cn } from "@/lib/utils";

/**
 * Snippet surfaces highlight in the browser rather than through a build-time
 * grammar engine: the product ships four snippet dialects, all of them short,
 * and a full highlighter would cost more bytes than every snippet combined.
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
  plain: "text-foreground/90",
  tag: "text-[var(--swatch-purple)]",
  attr: "text-[var(--swatch-blue)]",
  string: "text-[var(--swatch-green)]",
  comment: "text-muted-foreground/70 italic",
  punct: "text-muted-foreground",
  keyword: "text-[var(--swatch-pink)]",
  number: "text-[var(--swatch-orange)]",
  url: "text-[var(--swatch-blue)] underline decoration-border underline-offset-2",
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
  /** Eyebrow above the code, e.g. `README.md · SVG · dark`. */
  label?: ReactNode;
  copyLabel?: string;
  copyAriaLabel?: string;
  className?: string;
  /** Tailwind max-height utility applied to the scroll region. */
  maxHeightClass?: string;
};

/**
 * Snippet surface: a dithered bed, highlighted code, and the copy action that
 * every snippet in the product shares.
 */
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
    <div
      className={cn(
        "dither-fallback relative isolate overflow-hidden rounded-lg border border-border/60",
        className,
      )}
    >
      <DitherSurface fill={BRAND} variant="gradient" edge={null} alpha={0.16} />
      <div className="relative flex items-center justify-between gap-3 border-b border-border/40 px-3 py-2">
        <span className={cn(EYEBROW, "min-w-0 truncate")}>{label}</span>
        <CopyButton
          value={code}
          idleLabel={copyLabel}
          ariaLabel={copyAriaLabel ?? copyLabel}
          size="sm"
          className="shrink-0"
        />
      </div>
      <pre
        className={cn(
          "relative overflow-auto px-3 py-3 font-mono text-[12px] leading-relaxed",
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
