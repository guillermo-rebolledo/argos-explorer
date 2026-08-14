use std::path::PathBuf;

use argos_explorer::{
    app::{App, FileContent, Screen},
    config::{Config, IconMode, ThemeMode},
    ui,
    viewer::{LoadedDocument, load_document, text_document_from_string},
};
use ratatui::{Terminal, backend::TestBackend};

fn test_app(root: PathBuf) -> App {
    App::new(Config {
        root,
        config_path: PathBuf::from("config.toml"),
        icons: IconMode::None,
        theme: ThemeMode::Ansi,
        ascii: false,
        color: true,
        mouse: false,
        tab_width: 4,
        small_file_limit: 8 * 1024 * 1024,
        page_cache_bytes: 64 * 1024 * 1024,
        index_memory_bytes: 16 * 1024 * 1024,
        log_level: None,
    })
}

fn render_text(app: &mut App, width: u16, height: u16) -> String {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| {
            app.viewport_width = width as usize;
            app.viewport_height = height.saturating_sub(3) as usize;
            ui::render(frame, app);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn preview_replaces_files_screen_and_exposes_back_control() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let path = root.join("sample.rs");
    let mut app = test_app(root);
    app.screen = Screen::Preview;
    app.viewer.path = Some(path.clone());
    app.viewer.content =
        FileContent::Text(text_document_from_string(path, "fn main() {}\n".to_owned()));

    let screen = render_text(&mut app, 80, 20);
    assert!(screen.contains("[← Back]"));
    assert!(screen.contains("fn main() {}"));
    assert!(!screen.contains("Quick Open  [ Quit ]\nWorkspace:"));
}

#[test]
fn quick_open_back_restores_origin_screen() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.set_screen(Screen::Changes);
    app.open_quick_open();
    assert_eq!(app.screen, Screen::QuickOpen);
    app.back();
    assert_eq!(app.screen, Screen::Changes);
}

#[test]
fn escape_clears_search_before_leaving_screen() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.begin_search();
    app.search_char('r');
    app.back();

    assert_eq!(app.screen, Screen::Files);
    assert!(app.tree_filter.is_empty());
    assert!(app.search_mode.is_none());
}

#[test]
fn undersized_terminal_shows_explicit_fallback() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    let screen = render_text(&mut app, 30, 8);
    assert!(screen.contains("Terminal too small"));
}

#[test]
fn vscode_button_is_visible_only_when_cli_is_available() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.vscode_available = true;
    assert!(render_text(&mut app, 80, 20).contains("VSCode"));

    app.vscode_available = false;
    assert!(!render_text(&mut app, 80, 20).contains("VSCode"));
}

#[test]
fn narrow_top_bar_hides_vscode_to_preserve_quit_button() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.vscode_available = true;

    let screen = render_text(&mut app, 40, 20);

    assert!(!screen.contains("VSCode"));
    assert!(screen.contains("Quit"));
}

#[test]
fn markdown_preview_renders_semantics_and_searches_rendered_text() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let path = root.join("README.md");
    std::fs::write(
        &path,
        "# Preview title\n\nA **rendered paragraph** with [docs](https://example.com).\n",
    )
    .unwrap();
    let LoadedDocument::Markdown(document) = load_document(&path, 1024).unwrap() else {
        panic!("expected Markdown document");
    };
    let mut app = test_app(root);
    app.screen = Screen::Preview;
    app.viewer.path = Some(path);
    app.viewer.wrap = true;
    app.viewer.content = FileContent::Markdown(document);

    let screen = render_text(&mut app, 80, 20);
    assert!(screen.contains("Preview title"));
    assert!(screen.contains("rendered paragraph"));
    assert!(screen.contains("https://example.com"));
    assert!(!screen.contains("# Preview title"));
    assert!(!screen.contains("**rendered paragraph**"));

    app.begin_search();
    for character in "rendered paragraph".chars() {
        app.search_char(character);
    }
    assert!(!app.viewer.matches.is_empty());
}
