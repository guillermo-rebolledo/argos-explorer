use std::path::Path;

use crate::{config::IconMode, workspace::EntryKind};

pub fn icon(mode: IconMode, path: &Path, kind: EntryKind, expanded: bool) -> &'static str {
    match mode {
        IconMode::None => "",
        IconMode::NerdFont => nerd_font_icon(path, kind, expanded),
        IconMode::Emoji => emoji_icon(path, kind, expanded),
    }
}

fn nerd_font_icon(path: &Path, kind: EntryKind, expanded: bool) -> &'static str {
    if kind.is_directory() {
        return if expanded { "󰝰" } else { "󰉋" };
    }
    match extension(path) {
        "rs" => "",
        "toml" => "",
        "md" | "markdown" => "",
        "json" => "",
        "yaml" | "yml" => "",
        "js" | "jsx" => "",
        "ts" | "tsx" => "",
        "py" => "",
        "html" => "",
        "css" => "",
        "git" => "",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => "",
        "zip" | "tar" | "gz" | "7z" => "",
        _ => "󰈔",
    }
}

fn emoji_icon(path: &Path, kind: EntryKind, expanded: bool) -> &'static str {
    if kind.is_directory() {
        return if expanded { "📂" } else { "📁" };
    }
    match extension(path) {
        "rs" | "js" | "jsx" | "ts" | "tsx" | "py" | "html" | "css" => "📜",
        "md" | "markdown" | "txt" => "📄",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" => "🖼️",
        "zip" | "tar" | "gz" | "7z" => "📦",
        "json" | "toml" | "yaml" | "yml" => "⚙️",
        _ => "📄",
    }
}

fn extension(path: &Path) -> &str {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
}
