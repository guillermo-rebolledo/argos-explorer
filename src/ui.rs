use std::path::Path;

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{List, ListItem, Paragraph, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    app::{App, ChangeRow, FileContent, GitState, Screen},
    config::ThemeMode,
    git::ChangeGroup,
    icons,
    viewer::{HighlightedLine, Page, TextDocument},
    workspace::{EntryKind, LoadState},
};

const MIN_WIDTH: u16 = 40;
const MIN_HEIGHT: u16 = 10;

#[derive(Clone, Copy)]
struct Palette {
    text: Color,
    muted: Color,
    accent: Color,
    selection_fg: Color,
    selection_bg: Color,
    added: Color,
    removed: Color,
    warning: Color,
    error: Color,
}

pub fn render(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let message = format!(
            "Terminal too small\nNeed at least {MIN_WIDTH}×{MIN_HEIGHT}\nCurrent: {}×{}",
            area.width, area.height
        );
        frame.render_widget(Paragraph::new(message).alignment(Alignment::Center), area);
        return;
    }

    let palette = palette(app);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);
    render_tabs(frame, app, rows[0], palette);
    render_context(frame, app, rows[1], palette);
    match app.screen {
        Screen::Files => render_files(frame, app, rows[2], palette),
        Screen::Changes => render_changes(frame, app, rows[2], palette),
        Screen::Preview => render_preview(frame, app, rows[2], palette),
        Screen::Diff => render_diff(frame, app, rows[2], palette),
        Screen::QuickOpen => render_quick_open(frame, app, rows[2], palette),
        Screen::Help => render_help(frame, rows[2], palette),
    }
    render_status(frame, app, rows[3], palette);
}

fn render_tabs(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    let back = matches!(app.screen, Screen::Preview | Screen::Diff | Screen::Help);
    let mut spans = Vec::new();
    if back {
        spans.push(Span::styled(
            "[← Back]  ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(tab(" Files ", app.screen == Screen::Files, palette));
        spans.push(Span::raw(" "));
        spans.push(tab(" Changes ", app.screen == Screen::Changes, palette));
        spans.push(Span::raw(" "));
        spans.push(tab(
            " Quick Open ",
            app.screen == Screen::QuickOpen,
            palette,
        ));
        if app.vscode_available && area.width >= 48 {
            spans.push(Span::raw(" "));
            spans.push(action_button(" VSCode ", palette));
        }
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
    let quit_width = 8.min(area.width);
    let quit = Rect::new(
        area.right().saturating_sub(quit_width),
        area.y,
        quit_width,
        1,
    );
    frame.render_widget(
        Paragraph::new("[ Quit ]")
            .style(Style::default().fg(palette.muted))
            .alignment(Alignment::Right),
        quit,
    );
}

fn tab(label: &'static str, selected: bool, palette: Palette) -> Span<'static> {
    if selected {
        Span::styled(
            label,
            Style::default()
                .fg(palette.selection_fg)
                .bg(palette.selection_bg)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(label, Style::default().fg(palette.muted))
    }
}
fn action_button(label: &'static str, palette: Palette) -> Span<'static> {
    Span::styled(
        label,
        Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
    )
}

fn render_context(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    let text = if let Some((label, query)) = app.search_prompt() {
        format!("/{label}: {query}▌")
    } else {
        match app.screen {
            Screen::Files => format!("Workspace: {}", app.config.display_root()),
            Screen::Changes => app.git_status_label(),
            Screen::Preview => {
                let path = app
                    .viewer
                    .path
                    .as_ref()
                    .map(|path| relative(app, path))
                    .unwrap_or_else(|| "No file".to_owned());
                if app.viewer.stale {
                    format!("{path}  [changed on disk — press r to reload]")
                } else {
                    path
                }
            }
            Screen::Diff => app.diff.title.clone(),
            Screen::QuickOpen => format!("Quick Open: {}▌", app.quick_open.query()),
            Screen::Help => "Contextual Help".to_owned(),
        }
    };
    frame.render_widget(
        Paragraph::new(text).style(Style::default().fg(palette.accent)),
        area,
    );
}

fn render_files(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    let rows = app.visible_tree();
    let selected = app.tree.selected_path();
    let items = rows
        .iter()
        .skip(app.tree_scroll)
        .take(area.height as usize)
        .map(|entry| {
            let selected = selected == Some(entry.path.as_path());
            let cursor = if selected { ">" } else { " " };
            let indent = "  ".repeat(entry.depth);
            let disclosure = if entry.kind.is_directory() {
                if entry.expanded {
                    if app.config.ascii { "-" } else { "▾" }
                } else if app.config.ascii {
                    "+"
                } else {
                    "▸"
                }
            } else {
                " "
            };
            let icon = icons::icon(app.config.icons, &entry.path, entry.kind, entry.expanded);
            let load = match entry.load_state {
                LoadState::Loading => " …",
                LoadState::Failed => " !",
                LoadState::Unloaded | LoadState::Loaded => "",
            };
            let flags = format!(
                "{}{}",
                if entry.hidden { " [hidden]" } else { "" },
                if entry.ignored { " [ignored]" } else { "" }
            );
            let icon_cell = if icon.is_empty() {
                String::new()
            } else {
                icon_cell(icon)
            };
            let mut style = Style::default().fg(if entry.error.is_some() {
                palette.error
            } else if entry.hidden || entry.ignored {
                palette.muted
            } else {
                palette.text
            });
            if selected {
                style = style
                    .fg(palette.selection_fg)
                    .bg(palette.selection_bg)
                    .add_modifier(Modifier::BOLD);
            }
            ListItem::new(format!(
                "{cursor} {indent}{disclosure} {icon_cell}{}{flags}{load}",
                entry.name
            ))
            .style(style)
        })
        .collect::<Vec<_>>();
    let empty = rows.is_empty();
    frame.render_widget(List::new(items), area);
    if empty {
        frame.render_widget(
            Paragraph::new(if app.tree_filter.is_empty() {
                "This directory is empty or still loading."
            } else {
                "No loaded paths match the filter."
            })
            .style(Style::default().fg(palette.muted)),
            area,
        );
    }
}

fn render_changes(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    let rows = app.change_rows();
    if rows.is_empty() {
        let message = match &app.git_state {
            GitState::Discovering => "Discovering Git repository…",
            GitState::NotRepository => "Not a Git repository",
            GitState::Unavailable(error) => error.as_str(),
            GitState::Ready => "No changes",
        };
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(palette.muted))
                .alignment(Alignment::Center),
            area,
        );
        return;
    }

    let items = rows
        .iter()
        .skip(app.change_scroll)
        .take(area.height as usize)
        .map(|row| match row {
            ChangeRow::Header { group, count } => {
                ListItem::new(format!("{} ({count})", group.label())).style(
                    Style::default()
                        .fg(group_color(*group, palette))
                        .add_modifier(Modifier::BOLD),
                )
            }
            ChangeRow::Entry { index, entry } => {
                let selected = app.selected_change == Some(*index);
                let cursor = if selected { ">" } else { " " };
                let old = entry
                    .old_path
                    .as_ref()
                    .map(|path| format!("{} → ", path.display()))
                    .unwrap_or_default();
                let mut style = Style::default().fg(palette.text);
                if selected {
                    style = style
                        .fg(palette.selection_fg)
                        .bg(palette.selection_bg)
                        .add_modifier(Modifier::BOLD);
                }
                ListItem::new(format!(
                    "{cursor}  {} {old}{}",
                    entry.kind.marker(),
                    entry.path.display()
                ))
                .style(style)
            }
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), area);
}

fn render_preview(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    match &app.viewer.content {
        FileContent::Empty => render_center(frame, area, "No file selected", palette.muted),
        FileContent::Loading => render_center(frame, area, "Loading file…", palette.muted),
        FileContent::Error(error) => render_center(frame, area, error, palette.error),
        FileContent::Binary(document) => render_center(
            frame,
            area,
            &format!(
                "{}\nPath: {}\nSize: {} bytes",
                document.description,
                relative(app, &document.path),
                document.size
            ),
            palette.warning,
        ),
        FileContent::Text(document) => render_text_document(frame, area, document, app, palette),
        FileContent::Large(large) => {
            if let Some(page) = large.active_page() {
                render_large_page(frame, area, page, app, palette);
            } else {
                render_center(frame, area, "Loading page…", palette.muted);
            }
        }
    }
}

fn render_diff(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    if app.diff.loading {
        render_center(frame, area, "Loading diff…", palette.muted);
        return;
    }
    if let Some(error) = &app.diff.error {
        render_center(frame, area, error, palette.error);
        return;
    }
    if let Some(large) = &app.diff.large {
        if let Some(page) = large.active_page() {
            render_large_diff_page(frame, area, page, app, palette);
        } else {
            render_center(frame, area, "Loading diff page…", palette.muted);
        }
        return;
    }
    let Some(document) = &app.diff.document else {
        render_center(frame, area, "No diff selected", palette.muted);
        return;
    };

    let end = app
        .diff
        .vertical
        .saturating_add(area.height as usize)
        .min(document.line_count());
    let numbers = diff_numbers(document, app.diff.vertical, end);
    let lines = (app.diff.vertical..end)
        .zip(numbers)
        .map(|(index, (old, new))| {
            let source = document.line(index).unwrap_or_default();
            let cropped = crop(
                source,
                app.diff.horizontal,
                area.width.saturating_sub(16) as usize,
            );
            let color = if source.starts_with('+') && !source.starts_with("+++") {
                palette.added
            } else if source.starts_with('-') && !source.starts_with("---") {
                palette.removed
            } else if source.starts_with("@@") {
                palette.accent
            } else if source.starts_with("diff ")
                || source.starts_with("index ")
                || source.starts_with("---")
                || source.starts_with("+++")
            {
                palette.warning
            } else {
                palette.text
            };
            Line::from(vec![
                Span::styled(
                    format!("{:>6} {:>6} ", number(old), number(new)),
                    Style::default().fg(palette.muted),
                ),
                Span::styled(cropped, Style::default().fg(color)),
            ])
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(lines));
    if app.diff.wrap {
        frame.render_widget(paragraph.wrap(Wrap { trim: false }), area);
    } else {
        frame.render_widget(paragraph, area);
    }
}

fn render_quick_open(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    let results = app.quick_results();
    let items = results
        .iter()
        .enumerate()
        .map(|(visible_index, record)| {
            let absolute = app.quick_open.scroll().saturating_add(visible_index);
            let selected = absolute == app.quick_open.selected();
            let cursor = if selected { ">" } else { " " };
            let icon = icons::icon(
                app.config.icons,
                record.path.as_path(),
                EntryKind::File,
                false,
            );
            let icon = if icon.is_empty() {
                String::new()
            } else {
                icon_cell(icon)
            };
            let mut style = Style::default().fg(palette.text);
            if selected {
                style = style
                    .fg(palette.selection_fg)
                    .bg(palette.selection_bg)
                    .add_modifier(Modifier::BOLD);
            }
            ListItem::new(format!(
                "{cursor} {icon}{}",
                relative(app, record.path.as_path())
            ))
            .style(style)
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), area);
    if results.is_empty() {
        render_center(
            frame,
            area,
            if app.quick_open.is_indexing() {
                "Indexing Workspace…"
            } else {
                "No files match"
            },
            palette.muted,
        );
    }
}

fn render_help(frame: &mut Frame<'_>, area: Rect, palette: Palette) {
    let help = [
        (
            "Navigation",
            "↑/↓ or j/k move · Enter/left-click activate · ←/h collapse/back · →/l expand",
        ),
        (
            "Views",
            "Ctrl+1 Files · Ctrl+2 Changes · Ctrl+P Quick Open · Ctrl+O VSCode · Esc back · q quit",
        ),
        (
            "Search",
            "/ local filter/find · n/p next/previous match · Ctrl+n/Ctrl+p Quick Open selection",
        ),
        (
            "Viewer",
            "PageUp/PageDown · Home/End or g/G · w wrap · Shift+wheel horizontal",
        ),
        (
            "Diff",
            "[/] previous/next hunk when no text search is active",
        ),
        (
            "Refresh",
            "r active screen · Ctrl+r full Workspace and Git refresh · H reveal .git",
        ),
        ("Icons", "--icons nerd-font|emoji|none"),
    ];
    let lines = help
        .into_iter()
        .flat_map(|(title, body)| {
            [
                Line::from(Span::styled(
                    title,
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(body, Style::default().fg(palette.text))),
                Line::default(),
            ]
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(Text::from(lines)).wrap(Wrap { trim: false }),
        area,
    );
}

fn render_status(frame: &mut Frame<'_>, app: &App, area: Rect, palette: Palette) {
    let index = if app.quick_open.is_indexing() {
        format!("indexing {}", app.quick_open.indexed_count())
    } else if app.quick_open.is_partial() {
        format!("partial index {}", app.quick_open.indexed_count())
    } else {
        format!("{} files", app.quick_open.indexed_count())
    };
    let watch = if app.watcher_degraded {
        " · refresh degraded"
    } else {
        ""
    };
    let text = format!("{} · {index}{watch} · ? Help", app.status);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().fg(palette.muted))
            .alignment(Alignment::Left),
        area,
    );
}

fn render_text_document(
    frame: &mut Frame<'_>,
    area: Rect,
    document: &TextDocument,
    app: &App,
    palette: Palette,
) {
    let vertical = app.viewer.vertical;
    let horizontal = app.viewer.horizontal;
    let wrap = app.viewer.wrap;
    let line_number_width = document.line_count().max(1).to_string().len();
    let end = vertical
        .saturating_add(area.height as usize)
        .min(document.line_count());
    let lines = (vertical..end)
        .map(|index| {
            let prefix = Span::styled(
                format!("{:>width$} │ ", index + 1, width = line_number_width),
                Style::default().fg(palette.muted),
            );
            let available = area.width.saturating_sub((line_number_width + 3) as u16) as usize;
            let mut spans = vec![prefix];
            if app.config.color && horizontal == 0 {
                if let Some(highlighted) = document.highlighted.get(index) {
                    spans.extend(highlighted_spans(highlighted));
                } else {
                    spans.push(Span::raw(
                        document.line(index).unwrap_or_default().to_owned(),
                    ));
                }
            } else {
                spans.push(Span::styled(
                    crop(
                        document.line(index).unwrap_or_default(),
                        horizontal,
                        available,
                    ),
                    Style::default().fg(palette.text),
                ));
            }
            Line::from(spans)
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(lines));
    if wrap {
        frame.render_widget(paragraph.wrap(Wrap { trim: false }), area);
    } else {
        frame.render_widget(paragraph, area);
    }
}

fn render_large_page(frame: &mut Frame<'_>, area: Rect, page: &Page, app: &App, palette: Palette) {
    let text = &page.text;
    let ranges = &page.lines;
    let vertical = app.viewer.vertical;
    let horizontal = app.viewer.horizontal;
    let wrap = app.viewer.wrap;
    let prefix_label = format!("byte {}", page.offset);
    let end = vertical
        .saturating_add(area.height as usize)
        .min(ranges.len());
    let lines = (vertical..end)
        .map(|index| {
            let source = text.get(ranges[index].clone()).unwrap_or_default();
            let prefix = format!("{prefix_label}+{index:<5} │ ");
            Line::from(vec![
                Span::styled(prefix, Style::default().fg(palette.muted)),
                Span::styled(
                    crop(source, horizontal, area.width.saturating_sub(16) as usize),
                    Style::default().fg(palette.text),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(lines));
    if wrap {
        frame.render_widget(paragraph.wrap(Wrap { trim: false }), area);
    } else {
        frame.render_widget(paragraph, area);
    }
}

fn render_large_diff_page(
    frame: &mut Frame<'_>,
    area: Rect,
    page: &Page,
    app: &App,
    palette: Palette,
) {
    let end = app
        .diff
        .vertical
        .saturating_add(area.height as usize)
        .min(page.lines.len());
    let lines = (app.diff.vertical..end)
        .map(|index| {
            let source = page.line(index).unwrap_or_default();
            Line::from(vec![
                Span::styled(
                    format!("{:>6} {:>6} ", "", index + 1),
                    Style::default().fg(palette.muted),
                ),
                Span::styled("+", Style::default().fg(palette.added)),
                Span::styled(
                    crop(
                        source,
                        app.diff.horizontal,
                        area.width.saturating_sub(17) as usize,
                    ),
                    Style::default().fg(palette.added),
                ),
            ])
        })
        .collect::<Vec<_>>();
    let paragraph = Paragraph::new(Text::from(lines));
    if app.diff.wrap {
        frame.render_widget(paragraph.wrap(Wrap { trim: false }), area);
    } else {
        frame.render_widget(paragraph, area);
    }
}

fn highlighted_spans(line: &HighlightedLine) -> Vec<Span<'static>> {
    line.spans
        .iter()
        .map(|span| {
            let mut modifier = Modifier::empty();
            if span.bold {
                modifier |= Modifier::BOLD;
            }
            if span.italic {
                modifier |= Modifier::ITALIC;
            }
            if span.underline {
                modifier |= Modifier::UNDERLINED;
            }
            Span::styled(
                span.text.clone(),
                Style::default()
                    .fg(Color::Rgb(
                        span.foreground.r,
                        span.foreground.g,
                        span.foreground.b,
                    ))
                    .add_modifier(modifier),
            )
        })
        .collect()
}

fn render_center(frame: &mut Frame<'_>, area: Rect, message: &str, color: Color) {
    frame.render_widget(
        Paragraph::new(message)
            .style(Style::default().fg(color))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn diff_numbers(
    document: &TextDocument,
    start: usize,
    end: usize,
) -> Vec<(Option<u64>, Option<u64>)> {
    let mut old = 0_u64;
    let mut new = 0_u64;
    let mut in_hunk = false;
    let mut result = Vec::with_capacity(end.saturating_sub(start));
    for index in 0..end {
        let line = document.line(index).unwrap_or_default();
        if let Some((old_start, new_start)) = parse_hunk_header(line) {
            old = old_start;
            new = new_start;
            in_hunk = true;
            if index >= start {
                result.push((None, None));
            }
            continue;
        }
        let numbers = if !in_hunk || line.starts_with("\\ No newline") {
            (None, None)
        } else if line.starts_with('+') && !line.starts_with("+++") {
            let value = (None, Some(new));
            new += 1;
            value
        } else if line.starts_with('-') && !line.starts_with("---") {
            let value = (Some(old), None);
            old += 1;
            value
        } else {
            let value = (Some(old), Some(new));
            old += 1;
            new += 1;
            value
        };
        if index >= start {
            result.push(numbers);
        }
    }
    result
}

fn parse_hunk_header(line: &str) -> Option<(u64, u64)> {
    let line = line.strip_prefix("@@ -")?;
    let (old, rest) = line.split_once(" +")?;
    let (new, _) = rest.split_once(" @@")?;
    let old = old.split(',').next()?.parse().ok()?;
    let new = new.split(',').next()?.parse().ok()?;
    Some((old, new))
}

fn number(value: Option<u64>) -> String {
    value.map(|value| value.to_string()).unwrap_or_default()
}

fn crop(text: &str, horizontal: usize, width: usize) -> String {
    text.chars().skip(horizontal).take(width).collect()
}

fn icon_cell(icon: &str) -> String {
    let width = UnicodeWidthStr::width(icon);
    format!(
        "{icon}{} ",
        " ".repeat(2_usize.saturating_sub(width.min(2)))
    )
}

fn relative(app: &App, path: &Path) -> String {
    path.strip_prefix(&app.config.root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn group_color(group: ChangeGroup, palette: Palette) -> Color {
    match group {
        ChangeGroup::Conflict => palette.error,
        ChangeGroup::Staged => palette.added,
        ChangeGroup::Unstaged => palette.warning,
        ChangeGroup::Untracked => palette.accent,
    }
}

fn palette(app: &App) -> Palette {
    if !app.config.color || app.config.theme == ThemeMode::Monochrome {
        return Palette {
            text: Color::Reset,
            muted: Color::DarkGray,
            accent: Color::White,
            selection_fg: Color::Black,
            selection_bg: Color::White,
            added: Color::White,
            removed: Color::White,
            warning: Color::White,
            error: Color::White,
        };
    }
    match app.config.theme {
        ThemeMode::HighContrast => Palette {
            text: Color::White,
            muted: Color::Gray,
            accent: Color::LightCyan,
            selection_fg: Color::Black,
            selection_bg: Color::LightYellow,
            added: Color::LightGreen,
            removed: Color::LightRed,
            warning: Color::LightYellow,
            error: Color::LightRed,
        },
        ThemeMode::Ansi | ThemeMode::Monochrome => Palette {
            text: Color::Reset,
            muted: Color::DarkGray,
            accent: Color::Cyan,
            selection_fg: Color::Black,
            selection_bg: Color::Cyan,
            added: Color::Green,
            removed: Color::Red,
            warning: Color::Yellow,
            error: Color::LightRed,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewer::text_document_from_string;
    use std::path::PathBuf;

    #[test]
    fn parses_unified_hunk_header() {
        assert_eq!(parse_hunk_header("@@ -12,3 +20,4 @@ fn x"), Some((12, 20)));
    }

    #[test]
    fn calculates_old_and_new_line_numbers() {
        let document = text_document_from_string(
            PathBuf::from("file.rs"),
            "@@ -1,2 +1,2 @@\n-old\n+new\n same\n".to_owned(),
        );
        let numbers = diff_numbers(&document, 0, 4);
        assert_eq!(numbers[1], (Some(1), None));
        assert_eq!(numbers[2], (None, Some(1)));
        assert_eq!(numbers[3], (Some(2), Some(2)));
    }
}
