use std::path::PathBuf;

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

use super::{HighlightSpan, HighlightedLine, Rgb, TextEncoding};

const TEXT: Rgb = Rgb {
    r: 220,
    g: 220,
    b: 220,
};
const MUTED: Rgb = Rgb {
    r: 140,
    g: 140,
    b: 140,
};
const HEADING_ONE: Rgb = Rgb {
    r: 86,
    g: 156,
    b: 214,
};
const HEADING_OTHER: Rgb = Rgb {
    r: 120,
    g: 190,
    b: 230,
};
const CODE: Rgb = Rgb {
    r: 220,
    g: 170,
    b: 80,
};
const LINK: Rgb = Rgb {
    r: 78,
    g: 160,
    b: 230,
};
const QUOTE: Rgb = Rgb {
    r: 150,
    g: 170,
    b: 180,
};

#[derive(Debug, Clone)]
pub struct MarkdownDocument {
    pub path: PathBuf,
    pub encoding: TextEncoding,
    pub size: u64,
    pub source: String,
    pub plain_text: String,
    pub lines: Vec<HighlightedLine>,
}

impl MarkdownDocument {
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn line_text(&self, index: usize) -> Option<String> {
        self.lines
            .get(index)
            .map(|line| line.spans.iter().map(|span| span.text.as_str()).collect())
    }
}

#[derive(Debug, Clone)]
struct ListState {
    next: Option<u64>,
}

#[derive(Debug, Default)]
struct Renderer {
    lines: Vec<HighlightedLine>,
    current: Vec<HighlightSpan>,
    heading: Option<u8>,
    emphasis: usize,
    strong: usize,
    strikethrough: usize,
    superscript: usize,
    subscript: usize,
    code: usize,
    quote_depth: usize,
    lists: Vec<ListState>,
    links: Vec<String>,
    images: Vec<String>,
    table_header: usize,
    table_cell: usize,
    suppress_raw_html: usize,
}

impl Renderer {
    fn render(mut self, source: &str) -> Vec<HighlightedLine> {
        let options = Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_GFM
            | Options::ENABLE_DEFINITION_LIST
            | Options::ENABLE_MATH;
        for event in Parser::new_ext(source, options) {
            self.event(event);
        }
        self.finish_line(false);
        while self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.pop();
        }
        if self.lines.is_empty() {
            self.lines.push(HighlightedLine { spans: Vec::new() });
        }
        self.lines
    }

    fn event(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) if self.suppress_raw_html == 0 => self.append_text(&text),
            Event::Code(text) => {
                self.code += 1;
                self.append_text(&text);
                self.code -= 1;
            }
            Event::InlineMath(text) => {
                self.code += 1;
                self.append_text(&text);
                self.code -= 1;
            }
            Event::DisplayMath(text) => {
                self.blank_before_block();
                self.code += 1;
                self.append_text(&text);
                self.code -= 1;
                self.finish_line(false);
                self.blank_line();
            }
            Event::Html(_) | Event::InlineHtml(_) => {}
            Event::FootnoteReference(label) => self.append_text(&format!("[{label}]")),
            Event::SoftBreak => self.append_text(" "),
            Event::HardBreak => self.finish_line(false),
            Event::Rule => {
                self.finish_line(false);
                self.append_styled(
                    "────────────────────────",
                    MUTED,
                    false,
                    false,
                    false,
                    false,
                );
                self.finish_line(false);
                self.blank_line();
            }
            Event::TaskListMarker(checked) => {
                self.append_styled(
                    if checked { "[x] " } else { "[ ] " },
                    MUTED,
                    true,
                    false,
                    false,
                    false,
                );
            }
            Event::Text(_) => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.blank_before_block();
                self.heading = Some(heading_level(level));
            }
            Tag::BlockQuote(_) => {
                self.blank_before_block();
                self.quote_depth += 1;
            }
            Tag::CodeBlock(_) => {
                self.blank_before_block();
                self.code += 1;
            }
            Tag::HtmlBlock | Tag::MetadataBlock(_) => self.suppress_raw_html += 1,
            Tag::List(next) => {
                if self.lists.is_empty() {
                    self.blank_before_block();
                }
                self.lists.push(ListState { next });
            }
            Tag::Item => {
                self.finish_line(false);
                let depth = self.lists.len().saturating_sub(1);
                let prefix = match self.lists.last_mut().and_then(|list| list.next.as_mut()) {
                    Some(next) => {
                        let prefix = format!("{next}. ");
                        *next += 1;
                        prefix
                    }
                    None => "• ".to_owned(),
                };
                self.append_styled(
                    &format!("{}{prefix}", "  ".repeat(depth)),
                    MUTED,
                    true,
                    false,
                    false,
                    false,
                );
            }
            Tag::FootnoteDefinition(label) => {
                self.blank_before_block();
                self.append_styled(&format!("[{label}] "), MUTED, true, false, false, false);
            }
            Tag::DefinitionList => self.blank_before_block(),
            Tag::DefinitionListTitle => self.strong += 1,
            Tag::DefinitionListDefinition => {
                self.finish_line(false);
                self.append_styled("  ", MUTED, false, false, false, false);
            }
            Tag::Table(_) => self.blank_before_block(),
            Tag::TableHead => self.table_header += 1,
            Tag::TableRow => {
                self.finish_line(false);
                self.table_cell = 0;
            }
            Tag::TableCell => {
                if self.table_cell > 0 {
                    self.append_styled(" │ ", MUTED, false, false, false, false);
                }
                self.table_cell += 1;
            }
            Tag::Emphasis => self.emphasis += 1,
            Tag::Strong => self.strong += 1,
            Tag::Strikethrough => self.strikethrough += 1,
            Tag::Superscript => {
                self.superscript += 1;
                self.append_text("^");
            }
            Tag::Subscript => {
                self.subscript += 1;
                self.append_text("₍");
            }
            Tag::Link { dest_url, .. } => self.links.push(dest_url.into_string()),
            Tag::Image { dest_url, .. } => {
                self.images.push(dest_url.into_string());
                self.append_styled("Image: ", MUTED, true, false, false, false);
            }
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                self.finish_line(false);
                if self.lists.is_empty() {
                    self.blank_line();
                }
            }
            TagEnd::Heading(_) => {
                self.finish_line(false);
                self.heading = None;
                self.blank_line();
            }
            TagEnd::BlockQuote(_) => {
                self.finish_line(false);
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.blank_line();
            }
            TagEnd::CodeBlock => {
                self.finish_line(false);
                self.code = self.code.saturating_sub(1);
                self.blank_line();
            }
            TagEnd::HtmlBlock | TagEnd::MetadataBlock(_) => {
                self.suppress_raw_html = self.suppress_raw_html.saturating_sub(1);
            }
            TagEnd::List(_) => {
                self.finish_line(false);
                self.lists.pop();
                if self.lists.is_empty() {
                    self.blank_line();
                }
            }
            TagEnd::Item => self.finish_line(false),
            TagEnd::FootnoteDefinition => {
                self.finish_line(false);
                self.blank_line();
            }
            TagEnd::DefinitionList => self.blank_line(),
            TagEnd::DefinitionListTitle => {
                self.strong = self.strong.saturating_sub(1);
                self.finish_line(false);
            }
            TagEnd::DefinitionListDefinition => self.finish_line(false),
            TagEnd::Table => self.blank_line(),
            TagEnd::TableHead => self.table_header = self.table_header.saturating_sub(1),
            TagEnd::TableRow => self.finish_line(false),
            TagEnd::TableCell => {}
            TagEnd::Emphasis => self.emphasis = self.emphasis.saturating_sub(1),
            TagEnd::Strong => self.strong = self.strong.saturating_sub(1),
            TagEnd::Strikethrough => {
                self.strikethrough = self.strikethrough.saturating_sub(1);
            }
            TagEnd::Superscript => {
                self.append_text("^");
                self.superscript = self.superscript.saturating_sub(1);
            }
            TagEnd::Subscript => {
                self.append_text("₎");
                self.subscript = self.subscript.saturating_sub(1);
            }
            TagEnd::Link => {
                if let Some(destination) = self.links.pop() {
                    self.append_styled(
                        &format!(" ({destination})"),
                        LINK,
                        false,
                        false,
                        true,
                        false,
                    );
                }
            }
            TagEnd::Image => {
                if let Some(destination) = self.images.pop() {
                    self.append_styled(
                        &format!(" ({destination})"),
                        LINK,
                        false,
                        false,
                        true,
                        false,
                    );
                }
            }
        }
    }

    fn append_text(&mut self, text: &str) {
        let mut remaining = text;
        while let Some((line, rest)) = remaining.split_once('\n') {
            self.append_current_style(line.trim_end_matches('\r'));
            self.finish_line(false);
            remaining = rest;
        }
        if !remaining.is_empty() {
            self.append_current_style(remaining);
        }
    }

    fn append_current_style(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let foreground = if self.code > 0 {
            CODE
        } else if !self.links.is_empty() {
            LINK
        } else if self.heading == Some(1) {
            HEADING_ONE
        } else if self.heading.is_some() {
            HEADING_OTHER
        } else if self.quote_depth > 0 {
            QUOTE
        } else {
            TEXT
        };
        self.append_styled(
            text,
            foreground,
            self.strong > 0 || self.heading.is_some() || self.table_header > 0,
            self.emphasis > 0 || self.quote_depth > 0,
            !self.links.is_empty(),
            self.strikethrough > 0,
        );
    }

    fn append_styled(
        &mut self,
        text: &str,
        foreground: Rgb,
        bold: bool,
        italic: bool,
        underline: bool,
        strikethrough: bool,
    ) {
        if text.is_empty() {
            return;
        }
        self.ensure_quote_prefix();
        if let Some(last) = self.current.last_mut()
            && last.foreground == foreground
            && last.bold == bold
            && last.italic == italic
            && last.underline == underline
            && last.strikethrough == strikethrough
        {
            last.text.push_str(text);
            return;
        }
        self.current.push(HighlightSpan {
            text: text.to_owned(),
            foreground,
            bold,
            italic,
            underline,
            strikethrough,
        });
    }

    fn ensure_quote_prefix(&mut self) {
        if self.current.is_empty() && self.quote_depth > 0 {
            self.current.push(HighlightSpan {
                text: "│ ".repeat(self.quote_depth),
                foreground: MUTED,
                bold: false,
                italic: false,
                underline: false,
                strikethrough: false,
            });
        }
    }

    fn finish_line(&mut self, force: bool) {
        if self.current.is_empty() && !force {
            return;
        }
        self.lines.push(HighlightedLine {
            spans: std::mem::take(&mut self.current),
        });
    }

    fn blank_line(&mut self) {
        self.finish_line(false);
        if !self.lines.last().is_some_and(|line| line.spans.is_empty()) {
            self.lines.push(HighlightedLine { spans: Vec::new() });
        }
    }

    fn blank_before_block(&mut self) {
        self.finish_line(false);
        if !self.lines.is_empty() {
            self.blank_line();
        }
    }
}

pub fn render_markdown(
    path: PathBuf,
    encoding: TextEncoding,
    size: u64,
    source: &str,
) -> MarkdownDocument {
    let lines = Renderer::default().render(source);
    let mut plain_text = String::new();
    for (index, line) in lines.iter().enumerate() {
        if index > 0 {
            plain_text.push('\n');
        }
        for span in &line.spans {
            plain_text.push_str(&span.text);
        }
    }
    MarkdownDocument {
        path,
        encoding,
        size,
        plain_text,
        lines,
        source: source.to_owned(),
    }
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
    use super::*;

    #[test]
    fn renders_semantic_markdown_without_source_markers() {
        let source = "# Title\n\nA **strong** and *emphasized* [link](https://example.com).\n\n- first\n- [x] done\n\n```rust\nfn main() {}\n```\n";
        let document = render_markdown(
            PathBuf::from("README.md"),
            TextEncoding::Utf8,
            source.len() as u64,
            source,
        );

        assert!(document.plain_text.contains("Title"));
        assert!(!document.plain_text.contains("# Title"));
        assert!(!document.plain_text.contains("**strong**"));
        assert!(document.plain_text.contains("• first"));
        assert!(document.plain_text.contains("[x] done"));
        assert!(document.plain_text.contains("https://example.com"));
        assert!(document.plain_text.contains("fn main() {}"));
        assert!(
            document
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.bold)
        );
        assert!(
            document
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.italic)
        );
        assert!(
            document
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.underline)
        );
    }

    #[test]
    fn renders_quotes_tables_and_strikethrough() {
        let source =
            "> quoted text\n\n~~removed~~\n\n| Name | Value |\n| --- | --- |\n| one | two |\n";
        let document = render_markdown(
            PathBuf::from("guide.markdown"),
            TextEncoding::Utf8,
            source.len() as u64,
            source,
        );

        assert!(document.plain_text.contains("│ quoted text"));
        assert!(document.plain_text.contains("Name │ Value"));
        assert!(document.plain_text.contains("one │ two"));
        assert!(
            document
                .lines
                .iter()
                .flat_map(|line| &line.spans)
                .any(|span| span.strikethrough && span.text.contains("removed"))
        );
    }
}
