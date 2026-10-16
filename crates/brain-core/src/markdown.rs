//! A small Markdown reader for Claude's replies: the block and inline subset
//! Claude actually writes (headings, lists, code, quotes, tables, bold, code
//! spans, links). Rendering is left to the UI.

use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub enum Block {
    Heading { level: u8, text: Inline },
    Paragraph(Inline),
    /// `marker` is `•` for bullets or the number for ordered items (`2.`).
    Item { indent: usize, marker: String, text: Inline },
    Code { lang: Option<String>, text: String },
    Quote(Inline),
    Table { header: Vec<Inline>, rows: Vec<Vec<Inline>> },
    Rule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Span {
    Bold,
    Code,
    Link,
}

/// Text with Markdown markers removed and styled byte ranges into `text`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Inline {
    pub text: String,
    pub spans: Vec<(Range<usize>, Span)>,
}

pub fn parse(markdown: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut paragraph: Vec<&str> = Vec::new();
    let mut lines = markdown.lines().peekable();

    let flush = |paragraph: &mut Vec<&str>, blocks: &mut Vec<Block>| {
        if !paragraph.is_empty() {
            blocks.push(Block::Paragraph(inline(&paragraph.join("\n"))));
            paragraph.clear();
        }
    };

    while let Some(line) = lines.next() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();

        if let Some((marker, info)) = fence(trimmed) {
            flush(&mut paragraph, &mut blocks);
            let lang = Some(info.to_string()).filter(|l| !l.is_empty());
            // Fences nested inside (a `sql` block in a `markdown` block) open with an info
            // string and close bare; only the bare fence matching this one ends the block.
            let mut depth = 0;
            let mut code = Vec::new();
            for code_line in lines.by_ref() {
                if let Some((inner, inner_info)) = fence(code_line.trim_start()) {
                    if inner.starts_with(marker) && inner_info.is_empty() {
                        if depth == 0 {
                            break;
                        }
                        depth -= 1;
                    } else if inner.chars().next() == marker.chars().next() && !inner_info.is_empty() {
                        depth += 1;
                    }
                }
                code.push(strip_indent(code_line, indent));
            }
            blocks.push(Block::Code { lang, text: code.join("\n") });
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            continue;
        }
        if let Some((level, text)) = heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Heading { level, text: inline(text) });
            continue;
        }
        if matches!(trimmed, "---" | "***" | "___") {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Rule);
            continue;
        }
        if let Some((marker, text)) = list_item(trimmed) {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Item { indent: indent / 2, marker, text: inline(text) });
            continue;
        }
        if let Some(quote) = trimmed.strip_prefix('>') {
            flush(&mut paragraph, &mut blocks);
            blocks.push(Block::Quote(inline(quote.trim_start())));
            continue;
        }
        if trimmed.starts_with('|') {
            flush(&mut paragraph, &mut blocks);
            let mut table_lines = vec![trimmed];
            while let Some(next) = lines.peek() {
                if !next.trim_start().starts_with('|') {
                    break;
                }
                table_lines.push(lines.next().unwrap().trim_start());
            }
            blocks.push(table(&table_lines));
            continue;
        }
        paragraph.push(trimmed);
    }
    flush(&mut paragraph, &mut blocks);
    blocks
}

/// A code fence (three or more backticks or tildes) and its info string.
fn fence(line: &str) -> Option<(&str, &str)> {
    let ch = line.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = line.chars().take_while(|c| *c == ch).count();
    (len >= 3).then(|| (&line[..len], line[len..].trim()))
}

/// Removes up to `indent` leading spaces, the indentation of the opening fence.
fn strip_indent(line: &str, indent: usize) -> &str {
    let spaces = line.bytes().take(indent).take_while(|b| *b == b' ').count();
    &line[spaces..]
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    let rest = line[hashes..].strip_prefix(' ')?;
    (1..=6).contains(&hashes).then_some((hashes as u8, rest))
}

fn list_item(line: &str) -> Option<(String, &str)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(bullet) {
            return Some(("•".into(), rest));
        }
    }
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &line[digits..];
        if let Some(text) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((format!("{}.", &line[..digits]), text));
        }
    }
    None
}

fn table(lines: &[&str]) -> Block {
    let cells = |line: &str| -> Vec<Inline> {
        line.trim().trim_matches('|').split('|').map(|c| inline(c.trim())).collect()
    };
    let is_separator = |line: &str| line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
    let mut rows = lines.iter().filter(|l| !is_separator(l)).map(|l| cells(l));
    let header = rows.next().unwrap_or_default();
    Block::Table { header, rows: rows.collect() }
}

/// Strips `**bold**`, `` `code` `` and `[text](url)` markers, recording spans.
/// Unclosed markers stay literal.
pub fn inline(source: &str) -> Inline {
    let mut out = Inline::default();
    let mut rest = source;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**").filter(|e| *e > 0) {
                push_span(&mut out, &after[..end], Span::Bold);
                rest = &after[end + 2..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`').filter(|e| *e > 0) {
                push_span(&mut out, &after[..end], Span::Code);
                rest = &after[end + 1..];
                continue;
            }
        }
        if let Some(after) = rest.strip_prefix('[') {
            if let Some(close) = after.find("](") {
                if let Some(url_end) = after[close + 2..].find(')') {
                    push_span(&mut out, &after[..close], Span::Link);
                    rest = &after[close + 2 + url_end + 1..];
                    continue;
                }
            }
        }
        let next = rest
            .char_indices()
            .skip(1)
            .find(|(_, c)| matches!(c, '*' | '`' | '['))
            .map_or(rest.len(), |(i, _)| i);
        out.text.push_str(&rest[..next]);
        rest = &rest[next..];
    }
    out
}

fn push_span(out: &mut Inline, text: &str, span: Span) {
    let start = out.text.len();
    out.text.push_str(text);
    out.spans.push((start..out.text.len(), span));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_markers_become_spans_and_unclosed_ones_stay_literal() {
        let got = inline("Run **cargo test** in `crates/core`, see [docs](https://x.y) or 2 * 3");

        assert_eq!(got.text, "Run cargo test in crates/core, see docs or 2 * 3");
        let spans: Vec<(&str, Span)> = got.spans.iter().map(|(r, s)| (&got.text[r.clone()], *s)).collect();
        assert_eq!(spans, vec![("cargo test", Span::Bold), ("crates/core", Span::Code), ("docs", Span::Link)]);
    }

    #[test]
    fn parses_the_blocks_claude_writes() {
        let md = "## Ergebnis\n\nAlles **grün**.\nZweite Zeile.\n\n- eins\n  - unter\n2. zwei\n\n```rust\nfn main() {}\n```\n> Hinweis\n\n| A | B |\n|---|---|\n| 1 | `x` |\n---";

        let blocks = parse(md);

        assert_eq!(blocks.len(), 9);
        assert!(matches!(&blocks[0], Block::Heading { level: 2, text } if text.text == "Ergebnis"));
        assert!(matches!(&blocks[1], Block::Paragraph(p) if p.text == "Alles grün.\nZweite Zeile."));
        assert!(matches!(&blocks[2], Block::Item { indent: 0, marker, .. } if marker == "•"));
        assert!(matches!(&blocks[3], Block::Item { indent: 1, .. }));
        assert!(matches!(&blocks[4], Block::Item { marker, .. } if marker == "2."));
        assert!(matches!(&blocks[5], Block::Code { lang: Some(l), text } if l == "rust" && text == "fn main() {}"));
        assert!(matches!(&blocks[6], Block::Quote(q) if q.text == "Hinweis"));
        let Block::Table { header, rows } = &blocks[7] else { panic!("table") };
        assert_eq!(header.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(), vec!["A", "B"]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0][1].spans.len(), 1);
        assert_eq!(blocks[8], Block::Rule);
    }

    #[test]
    fn nested_fences_stay_inside_the_outer_code_block() {
        let md = "```markdown\n## Was\n\n  ```sql\n  SELECT 1;\n  ```\n\nEnde\n```\nDanach";

        let blocks = parse(md);

        assert_eq!(blocks.len(), 2);
        let Block::Code { lang: Some(lang), text } = &blocks[0] else { panic!("code") };
        assert_eq!(lang, "markdown");
        assert_eq!(text, "## Was\n\n  ```sql\n  SELECT 1;\n  ```\n\nEnde");
        assert!(matches!(&blocks[1], Block::Paragraph(p) if p.text == "Danach"));
    }

    #[test]
    fn longer_fences_and_indentation_follow_the_opening_fence() {
        let md = "  ````md\n  ```\n  x\n  ```\n  ````";

        let blocks = parse(md);

        assert_eq!(blocks, vec![Block::Code { lang: Some("md".into()), text: "```\nx\n```".into() }]);
    }

    #[test]
    fn handles_multibyte_text_around_markers() {
        let got = inline("Größe **über** `äöü` ✓");
        assert_eq!(got.text, "Größe über äöü ✓");
        assert_eq!(&got.text[got.spans[1].0.clone()], "äöü");
    }
}
