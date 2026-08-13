use std::path::PathBuf;

use argos_explorer::{
    app::{App, Screen},
    config::{Config, IconMode, ThemeMode},
    input::handle_event,
};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

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

#[test]
fn plain_one_selects_files_when_terminal_drops_control_modifier() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.set_screen(Screen::Changes);

    handle_event(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)),
    );

    assert_eq!(app.screen, Screen::Files);
}

#[test]
fn control_one_selects_files() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.set_screen(Screen::Changes);

    handle_event(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL)),
    );

    assert_eq!(app.screen, Screen::Files);
}

#[test]
fn plain_one_remains_search_text_in_filter_mode() {
    let temp = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(temp.path()).unwrap();
    let mut app = test_app(root);
    app.begin_search();

    handle_event(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::NONE)),
    );

    assert_eq!(app.screen, Screen::Files);
    assert_eq!(app.tree_filter, "1");
}
