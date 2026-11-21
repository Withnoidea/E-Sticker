use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::sync::OnceLock;
use syntect::{
    easy::HighlightLines, highlighting::ThemeSet, parsing::SyntaxSet, util::LinesWithEndings,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Paragraph,
    Heading(u8),
    Quote,
    Code,
    List,
    Table,
    Rule,
    Image,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub kind: Kind,
    pub text: String,
    pub language: Option<String>,
    pub spans: Vec<Span>,
    pub destination: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: u8,
    pub link: Option<String>,
    pub rgb: Option<(u8, u8, u8)>,
}

impl Block {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            text: String::new(),
            language: None,
            spans: Vec::new(),
            destination: None,
        }
    }

    fn push(&mut self, text: &str, style: u8, link: Option<&str>) {
        self.text.push_str(text);
        if let Some(last) = self.spans.last_mut()
            && last.style == style
            && last.link.as_deref() == link
        {
            last.text.push_str(text);
        } else {
            self.spans.push(Span {
                text: text.into(),
                style,
                link: link.map(str::to_owned),
                rgb: None,
            });
        }
    }
}

pub struct Insertion {
    pub text: String,
    pub selection_start: usize,
    pub selection_end: usize,
}

pub fn insertion(marker: &str, selected: &str, start: usize) -> Insertion {
    let selected = selected.replace("\r\n", "\n").replace('\n', "\r\n");
    let (text, prefix, length) = match marker {
        "**" | "*" | "~~" | "`" => (
            format!("{marker}{selected}{marker}"),
            marker.len(),
            selected.encode_utf16().count(),
        ),
        "```" => (
            format!("```\r\n{selected}\r\n```"),
            5,
            selected.encode_utf16().count(),
        ),
        "[]()" => {
            let label = if selected.is_empty() {
                "链接文字"
            } else {
                &selected
            };
            (
                format!("[{label}](url)"),
                label.encode_utf16().count() + 3,
                3,
            )
        }
        "![alt](path)" => {
            let label = if selected.is_empty() {
                "图片说明"
            } else {
                &selected
            };
            (
                format!("![{label}](path)"),
                label.encode_utf16().count() + 4,
                4,
            )
        }
        _ => {
            let marker = marker.replace("\r\n", "\n").replace('\n', "\r\n");
            let text = if marker.ends_with(' ') && !selected.is_empty() {
                selected
                    .split("\r\n")
                    .enumerate()
                    .map(|(index, line)| {
                        if marker == "1. " {
                            format!("{}. {line}", index + 1)
                        } else {
                            format!("{marker}{line}")
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\r\n")
            } else {
                format!("{marker}{selected}")
            };
            let caret = text.encode_utf16().count();
            (text, caret, 0)
        }
    };
    Insertion {
        text,
        selection_start: start + prefix,
        selection_end: start + prefix + length,
    }
}

pub struct OutlineEntry {
    pub level: u8,
    pub label: String,
    pub offset: usize,
}

pub fn outline(source: &str) -> Vec<OutlineEntry> {
    let mut entries = Vec::new();
    let mut current: Option<OutlineEntry> = None;
    let mut previous_byte = 0;
    let mut utf16_offset = 0;
    for (event, range) in Parser::new(source).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                utf16_offset += source[previous_byte..range.start].encode_utf16().count();
                previous_byte = range.start;
                current = Some(OutlineEntry {
                    level: heading_level(level),
                    label: String::new(),
                    offset: utf16_offset,
                });
            }
            Event::Text(value) | Event::Code(value) => {
                if let Some(entry) = &mut current {
                    entry.label.push_str(&value);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(entry) = &mut current {
                    entry.label.push(' ');
                }
            }
            Event::End(TagEnd::Heading(_)) => entries.extend(current.take()),
            _ => {}
        }
    }
    entries
}

pub fn parse(source: &str) -> Vec<Block> {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH;
    let mut blocks = Vec::new();
    let mut current: Option<Block> = None;
    let mut quote_depth = 0usize;
    let mut lists: Vec<Option<u64>> = Vec::new();
    let mut style = 0u8;
    let mut styles = Vec::new();
    let mut links: Vec<String> = Vec::new();
    let mut image: Option<Block> = None;
    for event in Parser::new_ext(source, options) {
        match event {
            Event::Start(Tag::BlockQuote(_)) => {
                flush(&mut current, &mut blocks);
                quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                flush(&mut current, &mut blocks);
                quote_depth = quote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                flush(&mut current, &mut blocks);
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                flush(&mut current, &mut blocks);
                lists.pop();
            }
            Event::Start(Tag::Paragraph) => {
                current.get_or_insert_with(|| {
                    Block::new(if quote_depth > 0 {
                        Kind::Quote
                    } else {
                        Kind::Paragraph
                    })
                });
            }
            Event::Start(Tag::Heading { level, .. }) => {
                flush(&mut current, &mut blocks);
                current = Some(Block::new(Kind::Heading(heading_level(level))));
            }
            Event::Start(Tag::Item) => {
                flush(&mut current, &mut blocks);
                let mut prefix = "  ".repeat(lists.len().saturating_sub(1));
                if let Some(Some(number)) = lists.last_mut() {
                    prefix.push_str(&format!("{number}. "));
                    *number = number.saturating_add(1);
                } else {
                    prefix.push_str("- ");
                }
                let mut block = Block::new(Kind::List);
                block.push(&prefix, 0, None);
                current = Some(block);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                flush(&mut current, &mut blocks);
                let mut block = Block::new(Kind::Code);
                if let pulldown_cmark::CodeBlockKind::Fenced(language) = kind {
                    block.language = language.split_whitespace().next().map(str::to_owned);
                }
                current = Some(block);
            }
            Event::Start(Tag::Table(_)) => {
                flush(&mut current, &mut blocks);
                current = Some(Block::new(Kind::Table));
            }
            Event::End(TagEnd::TableRow | TagEnd::TableHead) => {
                if let Some(block) = &mut current {
                    block.push("\n", 0, None);
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(block) = &mut current {
                    block.push("\t", 0, None);
                }
            }
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::Item
                | TagEnd::CodeBlock
                | TagEnd::Table,
            ) => flush(&mut current, &mut blocks),
            Event::Start(Tag::Image { dest_url, .. }) => {
                flush(&mut current, &mut blocks);
                let mut block = Block::new(Kind::Image);
                block.destination = Some(dest_url.to_string());
                image = Some(block);
            }
            Event::End(TagEnd::Image) => {
                if let Some(block) = image.take() {
                    blocks.push(block);
                }
            }
            Event::Start(Tag::Strong | Tag::Emphasis | Tag::Strikethrough) => {
                styles.push(style);
                style |= match event {
                    Event::Start(Tag::Strong) => 1,
                    Event::Start(Tag::Emphasis) => 2,
                    _ => 4,
                };
            }
            Event::End(TagEnd::Strong | TagEnd::Emphasis | TagEnd::Strikethrough) => {
                style = styles.pop().unwrap_or(0)
            }
            Event::Start(Tag::Link { dest_url, .. }) => links.push(dest_url.to_string()),
            Event::End(TagEnd::Link) => {
                links.pop();
            }
            Event::Text(value)
            | Event::Html(value)
            | Event::InlineHtml(value)
            | Event::InlineMath(value)
            | Event::DisplayMath(value) => {
                if let Some(block) = &mut image {
                    block.push(&value, 0, None);
                } else {
                    append(&mut current, quote_depth, &value, style, links.last());
                }
            }
            Event::Code(value) => {
                append(&mut current, quote_depth, &value, style | 8, links.last())
            }
            Event::TaskListMarker(checked) => append(
                &mut current,
                quote_depth,
                if checked { "[x] " } else { "[ ] " },
                style,
                None,
            ),
            Event::SoftBreak | Event::HardBreak => {
                append(&mut current, quote_depth, "\n", style, links.last())
            }
            Event::Rule => {
                flush(&mut current, &mut blocks);
                blocks.push(Block::new(Kind::Rule));
            }
            Event::FootnoteReference(label) => append(
                &mut current,
                quote_depth,
                &format!("[{label}]"),
                style,
                links.last(),
            ),
            _ => {}
        }
    }
    flush(&mut current, &mut blocks);
    blocks
}

pub fn highlight_code(source: &str, language: &str) -> Option<Vec<Span>> {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    static THEMES: OnceLock<ThemeSet> = OnceLock::new();
    let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);
    let themes = THEMES.get_or_init(ThemeSet::load_defaults);
    let syntax = syntaxes.find_syntax_by_token(language)?;
    let theme = &themes.themes["InspiredGitHub"];
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut highlighted: Vec<Span> = Vec::new();
    for line in LinesWithEndings::from(source) {
        for (style, text) in highlighter.highlight_line(line, syntaxes).ok()? {
            let rgb = Some((style.foreground.r, style.foreground.g, style.foreground.b));
            if let Some(last) = highlighted.last_mut()
                && last.rgb == rgb
            {
                last.text.push_str(text);
            } else {
                highlighted.push(Span {
                    text: text.to_owned(),
                    style: 8,
                    link: None,
                    rgb,
                });
            }
        }
    }
    Some(highlighted)
}

fn flush(current: &mut Option<Block>, blocks: &mut Vec<Block>) {
    if let Some(block) = current.take()
        && (!block.text.trim().is_empty() || matches!(block.kind, Kind::Rule | Kind::Image))
    {
        blocks.push(block);
    }
}

fn append(
    current: &mut Option<Block>,
    quote_depth: usize,
    value: &str,
    style: u8,
    link: Option<&String>,
) {
    current
        .get_or_insert_with(|| {
            Block::new(if quote_depth > 0 {
                Kind::Quote
            } else {
                Kind::Paragraph
            })
        })
        .push(value, style, link.map(String::as_str));
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fenced_code_languages_are_highlighted_without_changing_source() {
        let source = "```rust\nfn main() { let count = 42; }\n```\n";
        let block = super::parse(source)
            .into_iter()
            .find(|block| block.kind == super::Kind::Code)
            .unwrap();
        assert_eq!(block.language.as_deref(), Some("rust"));
        let spans = super::highlight_code(&block.text, block.language.as_deref().unwrap()).unwrap();
        assert_eq!(
            spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>(),
            block.text
        );
        assert!(
            spans
                .iter()
                .filter_map(|span| span.rgb)
                .collect::<std::collections::HashSet<_>>()
                .len()
                > 2
        );
        assert!(super::highlight_code("plain code", "unknown-language").is_none());
    }
    #[test]
    fn outline_uses_native_utf16_offsets_and_ignores_code_fences() {
        let prefix = "中文🙂\r\n\r\n```\r\n# not a heading\r\n```\r\n\r\n";
        let source = format!("{prefix}## **标题** `a`\r\n\r\nSetext\r\n======\r\n");
        let entries = super::outline(&source);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].level, 2);
        assert_eq!(entries[0].label, "标题 a");
        assert_eq!(entries[0].offset, prefix.encode_utf16().count());
        assert_eq!(entries[1].level, 1);
        assert_eq!(entries[1].label, "Setext");
    }
    use super::{Kind, parse};
    #[test]
    fn parses_common_mark_blocks_and_gfm_extensions() {
        let blocks = parse(
            "# title\n\n- [x] done\n- [ ] todo\n\n| A | B |\n| - | - |\n| 1 | 2 |\n\n~~deleted~~\n\n```rust\nlet x = 1;\n```\n",
        );
        assert_eq!(blocks[0].kind, Kind::Heading(1));
        assert!(blocks.iter().any(|block| block.text.contains("[x] done")));
        assert!(
            blocks
                .iter()
                .any(|block| block.kind == Kind::Table && block.text.contains("1\t2"))
        );
        assert!(
            blocks
                .iter()
                .any(|block| block.kind == Kind::Code && block.text.contains("let x = 1;"))
        );
    }
    #[test]
    fn raw_html_is_kept_as_plain_text() {
        let blocks = parse("<script>alert('no')</script>");
        assert!(blocks.iter().any(|block| block.text.contains("<script>")));
    }

    #[test]
    fn formatting_links_images_and_table_rows_are_preserved() {
        let blocks = parse(
            "**粗体** *斜体* ~~删除~~ `code` [链接](https://example.com)\n\n![图片](local.png)\n\n| A | B |\n| - | - |\n| 1 | 2 |\n",
        );
        let spans = &blocks[0].spans;
        assert!(
            spans
                .iter()
                .any(|span| span.text == "粗体" && span.style & 1 != 0)
        );
        assert!(
            spans
                .iter()
                .any(|span| span.text == "斜体" && span.style & 2 != 0)
        );
        assert!(
            spans
                .iter()
                .any(|span| span.text == "删除" && span.style & 4 != 0)
        );
        assert!(
            spans
                .iter()
                .any(|span| span.text == "code" && span.style & 8 != 0)
        );
        assert!(
            spans
                .iter()
                .any(|span| span.link.as_deref() == Some("https://example.com"))
        );
        assert!(blocks.iter().any(|block| block.kind == Kind::Image
            && block.destination.as_deref() == Some("local.png")
            && block.text == "图片"));
        let table = blocks
            .iter()
            .find(|block| block.kind == Kind::Table)
            .unwrap();
        assert_eq!(table.text.lines().count(), 2);
        assert_eq!(table.text.lines().next().unwrap(), "A\tB\t");
    }

    #[test]
    fn ordered_and_nested_lists_keep_their_content_and_numbers() {
        let blocks = parse("3. Third\n4. Fourth\n   - nested\n\n> quote\n\nlast");
        assert!(blocks.iter().any(|block| block.text == "3. Third"));
        assert!(blocks.iter().any(|block| block.text == "4. Fourth"));
        assert!(blocks.iter().any(|block| block.text.contains("nested")));
        assert!(
            blocks
                .iter()
                .any(|block| block.kind == Kind::Quote && block.text == "quote")
        );
        assert_eq!(blocks.last().unwrap().text, "last");
    }

    #[test]
    fn inline_insertion_selects_only_the_unicode_content() {
        for marker in ["**", "*", "~~", "`"] {
            let insertion = super::insertion(marker, "中文🙂", 4);
            assert_eq!(insertion.text, format!("{marker}中文🙂{marker}"));
            assert_eq!(insertion.selection_start, 4 + marker.len());
            assert_eq!(insertion.selection_end - insertion.selection_start, 4);
        }
    }

    #[test]
    fn link_and_image_insertions_select_the_exact_destination() {
        for (marker, destination) in [("[]()", "url"), ("![alt](path)", "path")] {
            let insertion = super::insertion(marker, "中文🙂", 0);
            let utf16: Vec<_> = insertion.text.encode_utf16().collect();
            assert_eq!(
                String::from_utf16_lossy(
                    &utf16[insertion.selection_start..insertion.selection_end]
                ),
                destination
            );
        }
    }

    #[test]
    fn block_insertions_use_native_crlf_and_preserve_line_selection() {
        let insertion = super::insertion("```", "甲\n乙", 0);
        assert_eq!(insertion.text, "```\r\n甲\r\n乙\r\n```");
        assert_eq!(insertion.selection_start, 5);
        assert_eq!(insertion.selection_end, 9);
        assert_eq!(
            super::insertion("1. ", "甲\r\n乙", 0).text,
            "1. 甲\r\n2. 乙"
        );
        assert_eq!(
            super::insertion("    ", "甲\r\n乙", 0).text,
            "    甲\r\n    乙"
        );
        assert_eq!(super::insertion("\n---\n", "", 0).text, "\r\n---\r\n");
    }
}
