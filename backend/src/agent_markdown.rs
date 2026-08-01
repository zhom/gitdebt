//! The text primitives every agent-facing document is built from.
//!
//! [`crate::agent_report`], [`crate::agent_pages`], [`crate::agent_docs`] and
//! [`crate::agent_prompt`] render one document family, so they share one copy
//! of the routines that decide what those documents look like: the header, the
//! bullet list, the fence, the GFM table, and the escaping each of those
//! requires.
//!
//! They live here rather than in [`crate::agent_embeds`] because that module is
//! a faithful port of `frontend/src/lib/readme-embeds.ts` with a cross-language
//! golden behind it; a Rust-only helper added there would make that claim false
//! and would have to be mirrored into TypeScript that has no use for it.
//!
//! The escaping is the reason this is one module and not four. GFM pipe
//! escaping and HTML attribute escaping are correctness routines: with a copy
//! per document, a fix lands in one document and silently leaves the others
//! publishing broken tables or invalid HTML.
//!
//! Everything here is pure: text in, text out, no clock and no database.

/// Sections joined into a finished document, one blank line apart, with the
/// single trailing newline every one of these surfaces ends on.
pub(crate) fn document(sections: &[String]) -> String {
    format!("{}\n", sections.join("\n\n"))
}

/// The opening of every document: the title, then the canonical HTML URL an
/// agent should cite instead of the Markdown it is reading.
pub(crate) fn document_header(title: &str, canonical: &str) -> String {
    format!("# {title}\n\nCanonical HTML: {canonical}")
}

pub(crate) fn bullet<I, S>(lines: I) -> String
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    lines
        .into_iter()
        .map(|line| format!("- {}", line.as_ref()))
        .collect::<Vec<_>>()
        .join("\n")
}

pub(crate) fn fence(language: &str, body: &str) -> String {
    format!("```{language}\n{body}\n```")
}

/// A fence that can hold fenced content of its own — the embedded agent prompt
/// carries snippets whose own fences would otherwise close the outer block.
pub(crate) fn outer_fence(language: &str, body: &str) -> String {
    format!("````{language}\n{body}\n````")
}

/// A GitHub-flavoured table. Pipes are escaped in every cell, including inside
/// code spans, which GFM requires — `theme=light|dark` or a path carrying a
/// pipe would otherwise open a phantom column and shift every later cell.
pub(crate) fn table(headers: &[&str], rows: &[Vec<String>]) -> String {
    let mut lines = vec![
        format!(
            "| {} |",
            headers
                .iter()
                .map(|value| cell(value))
                .collect::<Vec<_>>()
                .join(" | ")
        ),
        format!(
            "| {} |",
            headers
                .iter()
                .map(|_| "---")
                .collect::<Vec<_>>()
                .join(" | ")
        ),
    ];
    for row in rows {
        lines.push(format!(
            "| {} |",
            row.iter()
                .map(|value| cell(value))
                .collect::<Vec<_>>()
                .join(" | ")
        ));
    }
    lines.join("\n")
}

/// One table cell. A newline would end the row as surely as a pipe would end
/// the cell, so both are neutralized.
fn cell(value: &str) -> String {
    value.replace('|', "\\|").replace(['\n', '\r'], " ")
}

/// Escape a string on its way into an HTML attribute.
///
/// These documents publish `<picture>` snippets under prose telling the reader
/// to paste them into a README, so an unescaped value is shipped invalid HTML
/// rather than merely rendered oddly here. Hand-authored category names carry
/// ampersands today; a quote would break out of the attribute entirely.
pub(crate) fn escape_html_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            other => out.push(other),
        }
    }
    out
}

/// Trim a configured origin's trailing slash.
///
/// `api::normalize_origin` already guarantees a bare `scheme://host[:port]`,
/// so this never fires in production. It is applied to *both* origins in every
/// renderer anyway, because the asymmetry it replaces — site trimmed, api not —
/// is the shape a `https://host//api/md/...` bug hides in.
pub(crate) fn origin(raw: &str) -> &str {
    raw.trim_end_matches('/')
}

/// `12,043`. Agents quote these figures verbatim, so every agent-facing surface
/// groups them the same way.
pub(crate) fn thousands(value: i64) -> String {
    let digits = value.unsigned_abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if value < 0 {
        out.push('-');
    }
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_joins_sections_and_ends_on_one_newline() {
        assert_eq!(
            document(&["# Title".to_string(), "Body.".to_string()]),
            "# Title\n\nBody.\n"
        );
        assert_eq!(
            document_header("gitdebt", "https://gitdebt.com/"),
            "# gitdebt\n\nCanonical HTML: https://gitdebt.com/"
        );
    }

    #[test]
    fn bullet_and_fences_take_any_string_shape() {
        assert_eq!(bullet(["one", "two"]), "- one\n- two");
        assert_eq!(bullet(vec!["one".to_string()]), "- one");
        assert_eq!(fence("html", "<a />"), "```html\n<a />\n```");
        assert_eq!(
            outer_fence("markdown", "```\n```"),
            "````markdown\n```\n```\n````"
        );
    }

    /// The one copy of the escaping every table in every document depends on:
    /// a pipe opens a phantom column, a newline ends the row early.
    #[test]
    fn table_cells_escape_pipes_and_newlines() {
        let rendered = table(
            &["Parameter", "Effect"],
            &[vec![
                "`theme=light|dark`".to_string(),
                "one\ntwo".to_string(),
            ]],
        );
        assert_eq!(
            rendered,
            "| Parameter | Effect |\n| --- | --- |\n| `theme=light\\|dark` | one two |"
        );
        // A header is caller-supplied too.
        assert!(table(&["a|b"], &[]).contains("| a\\|b |"));
    }

    /// A value reaching an HTML attribute is escaped, or the snippet the
    /// document tells a reader to paste is not valid HTML.
    #[test]
    fn html_attributes_escape_every_breaking_character() {
        assert_eq!(
            escape_html_attribute(r#"Terminals & "multiplexers" <b>"#),
            "Terminals &amp; &quot;multiplexers&quot; &lt;b&gt;"
        );
        assert_eq!(escape_html_attribute("plain text"), "plain text");
    }

    #[test]
    fn origins_lose_a_trailing_slash_they_should_never_have_carried() {
        assert_eq!(origin("https://gitdebt.com/"), "https://gitdebt.com");
        assert_eq!(origin("https://gitdebt.com"), "https://gitdebt.com");
    }

    #[test]
    fn thousands_groups_both_signs() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(12_043), "12,043");
        assert_eq!(thousands(-1_000_000), "-1,000,000");
    }
}
