//! Line-based Markdown parser for the Documents app (P4-T13, DA-AC-05).
//!
//! The subset is deliberately small and predictable: headings (`#`..`######`),
//! paragraphs, flat bullet and numbered lists, fenced code blocks, quotes and
//! horizontal rules, with inline bold, italic, code and link text. It is a
//! renderer for the document viewer, not a full CommonMark implementation: no
//! nesting, tables or inline HTML.

/// One styled run inside a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    Text(String),
    Bold(String),
    Italic(String),
    Code(String),
    Link { text: String, url: String },
}

impl Span {
    pub fn text(&self) -> &str {
        match self {
            Span::Text(text) | Span::Bold(text) | Span::Italic(text) | Span::Code(text) => text,
            Span::Link { text, .. } => text,
        }
    }
}

/// One rendered block of the document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, spans: Vec<Span> },
    Paragraph { spans: Vec<Span> },
    Bullet { spans: Vec<Span> },
    Numbered { number: u32, spans: Vec<Span> },
    Code { language: String, lines: Vec<String> },
    Quote { spans: Vec<Span> },
    Divider,
}

impl Block {
    /// Unstyled text of the block, used by search, pagination and the UI rows.
    pub fn text(&self) -> String {
        match self {
            Block::Heading { spans, .. }
            | Block::Paragraph { spans }
            | Block::Bullet { spans }
            | Block::Numbered { spans, .. }
            | Block::Quote { spans } => plain_text(spans),
            Block::Code { lines, .. } => lines.join("\n"),
            Block::Divider => String::new(),
        }
    }

    pub fn spans(&self) -> &[Span] {
        match self {
            Block::Heading { spans, .. }
            | Block::Paragraph { spans }
            | Block::Bullet { spans }
            | Block::Numbered { spans, .. }
            | Block::Quote { spans } => spans,
            Block::Code { .. } | Block::Divider => &[],
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Block::Heading { .. } => "heading",
            Block::Paragraph { .. } => "paragraph",
            Block::Bullet { .. } => "bullet",
            Block::Numbered { .. } => "numbered",
            Block::Code { .. } => "code",
            Block::Quote { .. } => "quote",
            Block::Divider => "divider",
        }
    }
}

pub fn plain_text(spans: &[Span]) -> String {
    spans.iter().map(Span::text).collect::<Vec<_>>().join("")
}

/// Parse a whole document. `\r\n` and `\r` are normalised first.
pub fn parse(input: &str) -> Vec<Block> {
    let input = input.replace("\r\n", "\n").replace('\r', "\n");
    let mut blocks = Vec::new();
    let mut paragraph: Vec<String> = Vec::new();
    let mut quote: Vec<String> = Vec::new();
    let mut code: Option<(String, Vec<String>)> = None;

    fn flush(paragraph: &mut Vec<String>, blocks: &mut Vec<Block>) {
        if paragraph.is_empty() {
            return;
        }
        let text = paragraph.join(" ");
        paragraph.clear();
        blocks.push(Block::Paragraph {
            spans: parse_inline(&text),
        });
    }

    fn flush_quote(quote: &mut Vec<String>, blocks: &mut Vec<Block>) {
        if quote.is_empty() {
            return;
        }
        // A wrapped quote is one block, the way a wrapped paragraph is.
        let text = quote.join(" ");
        quote.clear();
        blocks.push(Block::Quote {
            spans: parse_inline(&text),
        });
    }

    for raw in input.split('\n') {
        // Inside a fenced block every line is kept verbatim.
        if let Some((language, lines)) = code.as_mut() {
            if raw.trim_start().starts_with("```") {
                blocks.push(Block::Code {
                    language: std::mem::take(language),
                    lines: std::mem::take(lines),
                });
                code = None;
            } else {
                lines.push(raw.to_string());
            }
            continue;
        }

        let trimmed = raw.trim();
        if trimmed.starts_with("```") {
            flush(&mut paragraph, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            code = Some((
                trimmed.trim_start_matches('`').trim().to_string(),
                Vec::new(),
            ));
            continue;
        }
        if trimmed.is_empty() {
            flush(&mut paragraph, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            continue;
        }
        if trimmed == "---" || trimmed == "***" || trimmed == "___" {
            flush(&mut paragraph, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Divider);
            continue;
        }
        if let Some((level, rest)) = heading(trimmed) {
            flush(&mut paragraph, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Heading {
                level,
                spans: parse_inline(rest),
            });
            continue;
        }
        if let Some(rest) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
            .or_else(|| trimmed.strip_prefix("+ "))
        {
            flush(&mut paragraph, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Bullet {
                spans: parse_inline(rest.trim()),
            });
            continue;
        }
        if let Some((number, rest)) = numbered(trimmed) {
            flush(&mut paragraph, &mut blocks);
            flush_quote(&mut quote, &mut blocks);
            blocks.push(Block::Numbered {
                number,
                spans: parse_inline(rest),
            });
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('>') {
            flush(&mut paragraph, &mut blocks);
            quote.push(rest.trim_start().to_string());
            continue;
        }
        flush_quote(&mut quote, &mut blocks);
        paragraph.push(trimmed.to_string());
    }

    // An unterminated fence still renders as code, never as lost text.
    if let Some((language, lines)) = code {
        blocks.push(Block::Code { language, lines });
    }
    flush_quote(&mut quote, &mut blocks);
    flush(&mut paragraph, &mut blocks);
    blocks
}

fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let rest = &line[hashes..];
    let rest = rest.strip_prefix(' ')?;
    Some((hashes as u8, rest.trim()))
}

fn numbered(line: &str) -> Option<(u32, &str)> {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 {
        return None;
    }
    let marker = line.get(digits..)?;
    let rest = marker.strip_prefix(". ")?;
    Some((line[..digits].parse().ok()?, rest.trim()))
}

/// Inline spans: `**bold**`, `*italic*`/`_italic_`, `` `code` `` and
/// `[text](url)`. Unmatched markers stay plain text.
pub fn parse_inline(text: &str) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut plain = String::new();
    let mut index = 0;

    while index < text.len() {
        let rest = &text[index..];

        if let Some(after) = rest.strip_prefix("**") {
            if let Some(end) = after.find("**") {
                if !after[..end].is_empty() {
                    if !plain.is_empty() {
                        spans.push(Span::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(Span::Bold(after[..end].to_string()));
                    index += 2 + end + 2;
                    continue;
                }
            }
        }

        if let Some(after) = rest.strip_prefix('`') {
            if let Some(end) = after.find('`') {
                if !plain.is_empty() {
                    spans.push(Span::Text(std::mem::take(&mut plain)));
                }
                spans.push(Span::Code(after[..end].to_string()));
                index += 1 + end + 1;
                continue;
            }
        }

        if rest.starts_with('[') {
            if let Some(close) = rest.find("](") {
                let label = &rest[1..close];
                let after = &rest[close + 2..];
                if let Some(paren) = after.find(')') {
                    if let Some(after_label) = after.get(..paren) {
                        if !plain.is_empty() {
                            spans.push(Span::Text(std::mem::take(&mut plain)));
                        }
                        spans.push(Span::Link {
                            text: label.to_string(),
                            url: after_label.to_string(),
                        });
                        index += close + 2 + paren + 1;
                        continue;
                    }
                }
            }
        }

        if rest.starts_with('*') || rest.starts_with('_') {
            let marker = rest.chars().next().expect("non-empty");
            let after = &rest[marker.len_utf8()..];
            if let Some(end) = after.find(marker) {
                if !after[..end].is_empty() {
                    if !plain.is_empty() {
                        spans.push(Span::Text(std::mem::take(&mut plain)));
                    }
                    spans.push(Span::Italic(after[..end].to_string()));
                    index += marker.len_utf8() + end + marker.len_utf8();
                    continue;
                }
            }
        }

        let character = rest.chars().next().expect("non-empty");
        plain.push(character);
        index += character.len_utf8();
    }

    if !plain.is_empty() {
        spans.push(Span::Text(plain));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_keep_their_level() {
        let blocks = parse("# Title\n## Section\n### Detail\n####### Not a heading\n#NoSpace");
        assert_eq!(blocks.len(), 4);
        match &blocks[0] {
            Block::Heading { level, spans } => {
                assert_eq!(*level, 1);
                assert_eq!(plain_text(spans), "Title");
            }
            other => panic!("expected heading, got {other:?}"),
        }
        assert_eq!(blocks[1], Block::Heading { level: 2, spans: vec![Span::Text("Section".into())] });
        assert_eq!(blocks[2].kind(), "heading");
        // Seven hashes is a paragraph, and so is a missing space.
        assert_eq!(blocks[3].kind(), "paragraph");
    }

    #[test]
    fn bullet_and_numbered_lists_are_flat_blocks() {
        let blocks = parse("- one\n- two with **bold**\n* three\n\n1. first\n2. second");
        assert_eq!(blocks.len(), 5);
        assert_eq!(
            blocks[0],
            Block::Bullet {
                spans: vec![Span::Text("one".into())]
            }
        );
        assert_eq!(plain_text(blocks[1].spans()), "two with bold");
        assert_eq!(blocks[2].kind(), "bullet");
        match &blocks[3] {
            Block::Numbered { number, spans } => {
                assert_eq!(*number, 1);
                assert_eq!(plain_text(spans), "first");
            }
            other => panic!("expected numbered item, got {other:?}"),
        }
        assert_eq!(blocks[4].kind(), "numbered");
    }

    #[test]
    fn fenced_code_blocks_keep_lines_and_language() {
        let blocks = parse("Intro\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\nAfter");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[0].kind(), "paragraph");
        match &blocks[1] {
            Block::Code { language, lines } => {
                assert_eq!(language, "rust");
                assert_eq!(lines.len(), 3);
                assert_eq!(lines[0], "fn main() {");
                assert_eq!(lines[2], "}");
            }
            other => panic!("expected code block, got {other:?}"),
        }
        assert_eq!(blocks[2].kind(), "paragraph");

        // An unterminated fence is still rendered as code.
        let blocks = parse("```\nlet x = 1;");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].kind(), "code");
        assert_eq!(blocks[0].text(), "let x = 1;");
    }

    #[test]
    fn mixed_document_parses_every_block_kind() {
        let source = "\
# Field notes

A paragraph that
wraps onto two lines.

> A quiet quote.
> Still the same quote.

---

## Lists
- one
1. first

```
code line
```
";
        let blocks = parse(source);
        let kinds: Vec<&str> = blocks.iter().map(Block::kind).collect();
        assert_eq!(
            kinds,
            vec![
                "heading",
                "paragraph",
                "quote",
                "divider",
                "heading",
                "bullet",
                "numbered",
                "code"
            ]
        );
        assert_eq!(
            blocks[2].text(),
            "A quiet quote. Still the same quote.",
            "wrapped quote lines merge like paragraphs"
        );
    }

    #[test]
    fn empty_and_whitespace_input_yields_no_blocks() {
        assert!(parse("").is_empty());
        assert!(parse("\n\n   \n\t\n").is_empty());
        // A lone divider is a real block, not empty output.
        assert_eq!(parse("---"), vec![Block::Divider]);
    }

    #[test]
    fn inline_spans_split_bold_italic_code_and_links() {
        let spans = parse_inline("Use **bold**, *italic*, `code` and [docs](https://aether.os).");
        assert_eq!(
            spans,
            vec![
                Span::Text("Use ".into()),
                Span::Bold("bold".into()),
                Span::Text(", ".into()),
                Span::Italic("italic".into()),
                Span::Text(", ".into()),
                Span::Code("code".into()),
                Span::Text(" and ".into()),
                Span::Link {
                    text: "docs".into(),
                    url: "https://aether.os".into()
                },
                Span::Text(".".into()),
            ]
        );
        assert_eq!(plain_text(&spans), "Use bold, italic, code and docs.");

        // Unmatched markers are literal text.
        assert_eq!(
            parse_inline("a **b"),
            vec![Span::Text("a **b".into())]
        );
        assert_eq!(parse_inline("_under_"), vec![Span::Italic("under".into())]);
        assert_eq!(parse_inline(""), Vec::new());
    }
}
