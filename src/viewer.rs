use std::{
    fs::{self, File},
    io::{BufReader, Read, Seek, SeekFrom},
    ops::Range,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use syntect::{
    easy::HighlightLines,
    highlighting::{Color as SyntectColor, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
use thiserror::Error;

mod markdown;
pub use markdown::MarkdownDocument;

pub const PAGE_SIZE: usize = 256 * 1024;
const BINARY_SAMPLE: usize = 8192;
const MAX_SEARCH_MATCHES: usize = 10_000;
static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
static THEMES: OnceLock<ThemeSet> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    Utf8,
    Utf8Lossy,
    Utf16Le,
    Utf16Be,
}

impl TextEncoding {
    pub fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::Utf8Lossy => "UTF-8 (lossy)",
            Self::Utf16Le => "UTF-16 LE",
            Self::Utf16Be => "UTF-16 BE",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl From<SyntectColor> for Rgb {
    fn from(value: SyntectColor) -> Self {
        Self {
            r: value.r,
            g: value.g,
            b: value.b,
        }
    }
}

#[derive(Debug, Clone)]
pub struct HighlightSpan {
    pub text: String,
    pub foreground: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone)]
pub struct HighlightedLine {
    pub spans: Vec<HighlightSpan>,
}

#[derive(Debug, Clone)]
pub struct TextDocument {
    pub path: PathBuf,
    pub encoding: TextEncoding,
    pub size: u64,
    pub text: String,
    pub line_ranges: Vec<Range<usize>>,
    pub highlighted: Vec<HighlightedLine>,
}

impl TextDocument {
    pub fn line(&self, index: usize) -> Option<&str> {
        self.line_ranges
            .get(index)
            .and_then(|range| self.text.get(range.clone()))
    }

    pub fn line_count(&self) -> usize {
        self.line_ranges.len()
    }
}

#[derive(Debug, Clone)]
pub struct BinaryDocument {
    pub path: PathBuf,
    pub size: u64,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct LargeDocument {
    pub path: PathBuf,
    pub size: u64,
    pub encoding: TextEncoding,
}

#[derive(Debug, Clone)]
pub enum LoadedDocument {
    Text(TextDocument),
    Markdown(MarkdownDocument),
    Binary(BinaryDocument),
    Large(LargeDocument),
}

#[derive(Debug, Clone)]
pub struct Page {
    pub offset: u64,
    pub next_offset: u64,
    pub text: String,
    pub lines: Vec<Range<usize>>,
    pub eof: bool,
}

impl Page {
    pub fn line(&self, index: usize) -> Option<&str> {
        self.lines
            .get(index)
            .and_then(|range| self.text.get(range.clone()))
    }
}

#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub byte_offset: u64,
    pub line_number: u64,
    pub excerpt: String,
}

#[derive(Debug, Error)]
pub enum ViewerError {
    #[error("could not inspect {path}: {source}")]
    Metadata {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not highlight {path}: {message}")]
    Highlight { path: PathBuf, message: String },
}

pub fn load_document(path: &Path, small_file_limit: u64) -> Result<LoadedDocument, ViewerError> {
    let metadata = fs::metadata(path).map_err(|source| ViewerError::Metadata {
        path: path.to_path_buf(),
        source,
    })?;
    let size = metadata.len();
    let mut file = File::open(path).map_err(|source| ViewerError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut sample = vec![0; BINARY_SAMPLE.min(size as usize)];
    let sample_len = file.read(&mut sample).map_err(|source| ViewerError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    sample.truncate(sample_len);
    let encoding = detect_encoding(&sample);
    let is_binary = !matches!(
        encoding,
        Some(TextEncoding::Utf16Le | TextEncoding::Utf16Be)
    ) && content_inspector::inspect(&sample)
        == content_inspector::ContentType::BINARY;
    if is_binary {
        return Ok(LoadedDocument::Binary(BinaryDocument {
            path: path.to_path_buf(),
            size,
            description: format!("Binary file\nSize: {size} bytes"),
        }));
    }

    let encoding = encoding.unwrap_or(TextEncoding::Utf8);
    if size > small_file_limit {
        return Ok(LoadedDocument::Large(LargeDocument {
            path: path.to_path_buf(),
            size,
            encoding,
        }));
    }

    file.seek(SeekFrom::Start(0))
        .map_err(|source| ViewerError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let mut bytes = Vec::with_capacity(size as usize);
    file.read_to_end(&mut bytes)
        .map_err(|source| ViewerError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let (encoding, text) = decode(&bytes, encoding);
    if is_markdown(path) {
        return Ok(LoadedDocument::Markdown(markdown::render_markdown(
            path.to_path_buf(),
            encoding,
            size,
            &text,
        )));
    }
    let line_ranges = line_ranges(&text);
    let highlighted = highlight(path, &text).unwrap_or_default();
    Ok(LoadedDocument::Text(TextDocument {
        path: path.to_path_buf(),
        encoding,
        size,
        text,
        line_ranges,
        highlighted,
    }))
}

pub fn text_document_from_string(path: PathBuf, text: String) -> TextDocument {
    let size = text.len() as u64;
    let line_ranges = line_ranges(&text);
    let highlighted = highlight(&path, &text).unwrap_or_default();
    TextDocument {
        path,
        encoding: TextEncoding::Utf8,
        size,
        text,
        line_ranges,
        highlighted,
    }
}

fn is_markdown(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "md" | "markdown" | "mdown" | "mkd" | "mkdn"
            )
        })
}

pub fn load_page(
    document: &LargeDocument,
    requested_offset: u64,
    page_size: usize,
) -> Result<Page, ViewerError> {
    let alignment = if matches!(
        document.encoding,
        TextEncoding::Utf16Le | TextEncoding::Utf16Be
    ) {
        2
    } else {
        1
    };
    let offset = requested_offset - requested_offset % alignment;
    let mut file = File::open(&document.path).map_err(|source| ViewerError::Read {
        path: document.path.clone(),
        source,
    })?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|source| ViewerError::Read {
            path: document.path.clone(),
            source,
        })?;
    let remaining = document.size.saturating_sub(offset);
    let length = remaining.min(page_size as u64) as usize;
    let mut bytes = vec![0; length];
    file.read_exact(&mut bytes)
        .map_err(|source| ViewerError::Read {
            path: document.path.clone(),
            source,
        })?;
    let (_, text) = decode_page(&bytes, document.encoding, offset == 0);
    let lines = line_ranges(&text);
    Ok(Page {
        offset,
        next_offset: offset.saturating_add(length as u64),
        text,
        lines,
        eof: offset.saturating_add(length as u64) >= document.size,
    })
}

pub fn search_large_file(path: &Path, query: &str) -> Result<Vec<SearchMatch>, ViewerError> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut file = File::open(path).map_err(|source| ViewerError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let mut signature = [0_u8; 2];
    let signature_len = file
        .read(&mut signature)
        .map_err(|source| ViewerError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    file.seek(SeekFrom::Start(0))
        .map_err(|source| ViewerError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    if signature_len == 2 && signature == [0xff, 0xfe] {
        search_utf16(path, file, query, true)
    } else if signature_len == 2 && signature == [0xfe, 0xff] {
        search_utf16(path, file, query, false)
    } else {
        search_utf8(path, file, query)
    }
}

fn search_utf8(path: &Path, file: File, query: &str) -> Result<Vec<SearchMatch>, ViewerError> {
    let mut reader = BufReader::with_capacity(PAGE_SIZE, file);
    let mut buffer = vec![0_u8; 64 * 1024];
    let mut state = StreamingSearch::new(query, 0);
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| ViewerError::Read {
                path: path.to_path_buf(),
                source,
            })?;
        if read == 0 {
            break;
        }
        let mut start = 0;
        for newline in memchr::memchr_iter(b'\n', &buffer[..read]) {
            state.feed(&String::from_utf8_lossy(&buffer[start..newline]));
            state.finish_line((newline + 1 - start) as u64);
            if state.matches.len() >= MAX_SEARCH_MATCHES {
                return Ok(state.matches);
            }
            start = newline + 1;
        }
        if start < read {
            state.feed(&String::from_utf8_lossy(&buffer[start..read]));
            state.advance((read - start) as u64);
        }
    }
    state.finish_eof();
    Ok(state.matches)
}

fn search_utf16(
    path: &Path,
    mut file: File,
    query: &str,
    little_endian: bool,
) -> Result<Vec<SearchMatch>, ViewerError> {
    file.seek(SeekFrom::Start(2))
        .map_err(|source| ViewerError::Read {
            path: path.to_path_buf(),
            source,
        })?;
    let mut reader = BufReader::with_capacity(PAGE_SIZE, file);
    let mut state = StreamingSearch::new(query, 2);
    let mut pair = [0_u8; 2];
    let mut units = Vec::with_capacity(4096);
    loop {
        match reader.read_exact(&mut pair) {
            Ok(()) => {
                let unit = if little_endian {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                };
                if unit == b'\n' as u16 {
                    state.feed(&String::from_utf16_lossy(&units));
                    units.clear();
                    state.finish_line(2);
                    if state.matches.len() >= MAX_SEARCH_MATCHES {
                        return Ok(state.matches);
                    }
                } else {
                    units.push(unit);
                    state.advance(2);
                    if units.len() >= 4096 {
                        state.feed(&String::from_utf16_lossy(&units));
                        units.clear();
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(source) => {
                return Err(ViewerError::Read {
                    path: path.to_path_buf(),
                    source,
                });
            }
        }
    }
    if !units.is_empty() {
        state.feed(&String::from_utf16_lossy(&units));
    }
    state.finish_eof();
    Ok(state.matches)
}

struct StreamingSearch {
    query: String,
    tail: String,
    excerpt: String,
    matched: bool,
    line_start: u64,
    line_number: u64,
    current_offset: u64,
    matches: Vec<SearchMatch>,
}

impl StreamingSearch {
    fn new(query: &str, start_offset: u64) -> Self {
        Self {
            query: query.to_lowercase(),
            tail: String::new(),
            excerpt: String::new(),
            matched: false,
            line_start: start_offset,
            line_number: 1,
            current_offset: start_offset,
            matches: Vec::new(),
        }
    }

    fn feed(&mut self, text: &str) {
        if self.excerpt.chars().count() < 240 {
            let remaining = 240 - self.excerpt.chars().count();
            self.excerpt.extend(text.chars().take(remaining));
        }
        let combined = format!("{}{}", self.tail, text);
        if combined.to_lowercase().contains(&self.query) {
            self.matched = true;
        }
        let keep = self
            .query
            .chars()
            .count()
            .saturating_mul(2)
            .saturating_add(8);
        let mut tail: Vec<char> = combined.chars().rev().take(keep).collect();
        tail.reverse();
        self.tail = tail.into_iter().collect();
    }

    fn advance(&mut self, bytes: u64) {
        self.current_offset = self.current_offset.saturating_add(bytes);
    }

    fn finish_line(&mut self, newline_bytes: u64) {
        self.current_offset = self.current_offset.saturating_add(newline_bytes);
        if self.matched {
            self.matches.push(SearchMatch {
                byte_offset: self.line_start,
                line_number: self.line_number,
                excerpt: self.excerpt.trim_end_matches('\r').to_owned(),
            });
        }
        self.line_start = self.current_offset;
        self.line_number += 1;
        self.tail.clear();
        self.excerpt.clear();
        self.matched = false;
    }

    fn finish_eof(&mut self) {
        if self.matched && self.matches.len() < MAX_SEARCH_MATCHES {
            self.matches.push(SearchMatch {
                byte_offset: self.line_start,
                line_number: self.line_number,
                excerpt: self.excerpt.clone(),
            });
        }
    }
}

pub fn find_lines(text: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let query = query.to_lowercase();
    text.lines()
        .enumerate()
        .filter_map(|(index, line)| line.to_lowercase().contains(&query).then_some(index))
        .collect()
}

fn detect_encoding(sample: &[u8]) -> Option<TextEncoding> {
    if sample.starts_with(&[0xff, 0xfe]) {
        Some(TextEncoding::Utf16Le)
    } else if sample.starts_with(&[0xfe, 0xff]) {
        Some(TextEncoding::Utf16Be)
    } else if sample.starts_with(&[0xef, 0xbb, 0xbf]) {
        Some(TextEncoding::Utf8)
    } else {
        None
    }
}

fn decode(bytes: &[u8], requested: TextEncoding) -> (TextEncoding, String) {
    decode_page(bytes, requested, true)
}

fn decode_page(bytes: &[u8], requested: TextEncoding, first_page: bool) -> (TextEncoding, String) {
    match requested {
        TextEncoding::Utf16Le => {
            let start = usize::from(first_page && bytes.starts_with(&[0xff, 0xfe])) * 2;
            let words: Vec<u16> = bytes[start..]
                .chunks_exact(2)
                .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
                .collect();
            (TextEncoding::Utf16Le, String::from_utf16_lossy(&words))
        }
        TextEncoding::Utf16Be => {
            let start = usize::from(first_page && bytes.starts_with(&[0xfe, 0xff])) * 2;
            let words: Vec<u16> = bytes[start..]
                .chunks_exact(2)
                .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
                .collect();
            (TextEncoding::Utf16Be, String::from_utf16_lossy(&words))
        }
        TextEncoding::Utf8 | TextEncoding::Utf8Lossy => {
            let bytes = if first_page && bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
                &bytes[3..]
            } else {
                bytes
            };
            match String::from_utf8(bytes.to_vec()) {
                Ok(text) => (TextEncoding::Utf8, text),
                Err(error) => (
                    TextEncoding::Utf8Lossy,
                    String::from_utf8_lossy(error.as_bytes()).into_owned(),
                ),
            }
        }
    }
}

fn line_ranges(text: &str) -> Vec<Range<usize>> {
    if text.is_empty() {
        return std::iter::once(0..0).collect();
    }
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            let mut end = index;
            if end > start && text.as_bytes()[end - 1] == b'\r' {
                end -= 1;
            }
            ranges.push(start..end);
            start = index + 1;
        }
    }
    if start < text.len() || text.ends_with('\n') {
        ranges.push(start..text.len());
    }
    ranges
}

fn highlight(path: &Path, text: &str) -> Result<Vec<HighlightedLine>, ViewerError> {
    let syntaxes = SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines);
    let themes = THEMES.get_or_init(ThemeSet::load_defaults);
    let syntax = syntaxes
        .find_syntax_for_file(path)
        .map_err(|error| ViewerError::Highlight {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text());
    let Some(theme) = themes
        .themes
        .get("base16-ocean.dark")
        .or_else(|| themes.themes.values().next())
    else {
        return Ok(Vec::new());
    };
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut result = Vec::new();
    for line in LinesWithEndings::from(text) {
        let spans = highlighter
            .highlight_line(line, syntaxes)
            .map_err(|error| ViewerError::Highlight {
                path: path.to_path_buf(),
                message: error.to_string(),
            })?
            .into_iter()
            .map(|(style, text)| HighlightSpan {
                text: text.trim_end_matches(['\r', '\n']).to_owned(),
                foreground: style.foreground.into(),
                bold: style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::BOLD),
                italic: style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::ITALIC),
                underline: style
                    .font_style
                    .contains(syntect::highlighting::FontStyle::UNDERLINE),
                strikethrough: false,
            })
            .collect();
        result.push(HighlightedLine { spans });
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_ranges_remove_crlf_from_display() {
        let text = "one\r\ntwo\n";
        let ranges = line_ranges(text);
        assert_eq!(&text[ranges[0].clone()], "one");
        assert_eq!(&text[ranges[1].clone()], "two");
    }

    #[test]
    fn utf16_little_endian_is_decoded() {
        let bytes = [0xff, 0xfe, b'h', 0, b'i', 0];
        let (_, text) = decode(&bytes, TextEncoding::Utf16Le);
        assert_eq!(text, "hi");
    }
}
