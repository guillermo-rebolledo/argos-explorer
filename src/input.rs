use crossterm::event::{
    Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};

use crate::app::{App, Screen};

pub fn handle_event(app: &mut App, event: Event) {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => handle_key(app, key),
        Event::Mouse(mouse) => handle_mouse(app, mouse),
        Event::Resize(_, _) => {}
        Event::FocusGained | Event::FocusLost | Event::Paste(_) | Event::Key(_) => {}
    }
}

fn handle_key(app: &mut App, key: KeyEvent) {
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        match key.code {
            KeyCode::Char('c') => {
                app.quit();
                return;
            }
            KeyCode::Char('o') => {
                app.open_vscode();
                return;
            }
            KeyCode::Char('p') => {
                if app.screen == Screen::QuickOpen {
                    app.move_selection(-1);
                } else {
                    app.open_quick_open();
                }
                return;
            }
            KeyCode::Char('n') if app.screen == Screen::QuickOpen => {
                app.move_selection(1);
                return;
            }
            KeyCode::Char('1') => {
                app.set_screen(Screen::Files);
                return;
            }
            KeyCode::Char('2') => {
                app.set_screen(Screen::Changes);
                return;
            }
            KeyCode::Char('r') => {
                app.full_refresh();
                return;
            }
            _ => {}
        }
    }

    if app.search_mode.is_some() {
        match key.code {
            KeyCode::Esc => app.back(),
            KeyCode::Enter => app.finish_search(),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Char(value) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.search_char(value)
            }
            _ => {}
        }
        return;
    }

    if app.screen == Screen::QuickOpen {
        match key.code {
            KeyCode::Esc => app.back(),
            KeyCode::Enter => app.activate(),
            KeyCode::Backspace => app.search_backspace(),
            KeyCode::Up => app.move_selection(-1),
            KeyCode::Down => app.move_selection(1),
            KeyCode::PageUp => app.page(-1),
            KeyCode::PageDown => app.page(1),
            KeyCode::Home => app.home(),
            KeyCode::End => app.end(),
            KeyCode::Char(value) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                app.search_char(value)
            }
            _ => {}
        }
        return;
    }

    match key.code {
        KeyCode::Char('1') => app.set_screen(Screen::Files),
        KeyCode::Char('2') => app.set_screen(Screen::Changes),
        KeyCode::Char('q') => app.quit(),
        KeyCode::Esc => app.back(),
        KeyCode::Char('?') => app.open_help(),
        KeyCode::Char('/') => app.begin_search(),
        KeyCode::Enter => app.activate(),
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1),
        KeyCode::Right | KeyCode::Char('l') => app.expand(),
        KeyCode::Left | KeyCode::Char('h') => app.collapse_or_back(),
        KeyCode::PageUp => app.page(-1),
        KeyCode::PageDown => app.page(1),
        KeyCode::Home | KeyCode::Char('g') => app.home(),
        KeyCode::End | KeyCode::Char('G') => app.end(),
        KeyCode::Char('w') => app.toggle_wrap(),
        KeyCode::Char('r') => app.reload_active(),
        KeyCode::Char('H') => app.toggle_git_directory(),
        KeyCode::Char('n') | KeyCode::Char(']') => app.next_match_or_hunk(1),
        KeyCode::Char('p') | KeyCode::Char('[') => app.next_match_or_hunk(-1),
        KeyCode::Tab => match app.screen {
            Screen::Files => app.set_screen(Screen::Changes),
            Screen::Changes => app.set_screen(Screen::Files),
            _ => {}
        },
        _ => {}
    }
}

fn handle_mouse(app: &mut App, mouse: MouseEvent) {
    match mouse.kind {
        MouseEventKind::Down(MouseButton::Left) => app.click(mouse.column, mouse.row),
        MouseEventKind::ScrollUp => {
            if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                app.horizontal_scroll(-4);
            } else {
                app.move_selection(-3);
            }
        }
        MouseEventKind::ScrollDown => {
            if mouse.modifiers.contains(KeyModifiers::SHIFT) {
                app.horizontal_scroll(4);
            } else {
                app.move_selection(3);
            }
        }
        MouseEventKind::ScrollLeft => app.horizontal_scroll(-4),
        MouseEventKind::ScrollRight => app.horizontal_scroll(4),
        MouseEventKind::Down(_)
        | MouseEventKind::Up(_)
        | MouseEventKind::Drag(_)
        | MouseEventKind::Moved => {}
    }
}
