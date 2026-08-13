use std::{
    io::{self, Stdout, stdout},
    panic,
    sync::{
        Once,
        atomic::{AtomicBool, Ordering},
    },
};

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};

static HOOK: Once = Once::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static MOUSE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub type ArgosExplorerTerminal = Terminal<CrosstermBackend<Stdout>>;

pub struct TerminalSession {
    terminal: ArgosExplorerTerminal,
}

impl TerminalSession {
    pub fn enter(mouse: bool) -> io::Result<Self> {
        install_panic_hook();
        enable_raw_mode()?;
        ACTIVE.store(true, Ordering::Release);
        MOUSE_ACTIVE.store(mouse, Ordering::Release);

        let mut output = stdout();
        if let Err(error) = execute!(output, EnterAlternateScreen, Hide) {
            restore_active_terminal();
            return Err(error);
        }
        if mouse && let Err(error) = execute!(output, EnableMouseCapture) {
            restore_active_terminal();
            return Err(error);
        }

        let backend = CrosstermBackend::new(output);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        Ok(Self { terminal })
    }

    pub fn terminal_mut(&mut self) -> &mut ArgosExplorerTerminal {
        &mut self.terminal
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        restore_active_terminal();
    }
}

fn install_panic_hook() {
    HOOK.call_once(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info| {
            restore_active_terminal();
            previous(info);
        }));
    });
}

fn restore_active_terminal() {
    if !ACTIVE.swap(false, Ordering::AcqRel) {
        return;
    }

    let mouse = MOUSE_ACTIVE.swap(false, Ordering::AcqRel);
    let mut output = stdout();
    if mouse {
        let _ = execute!(output, DisableMouseCapture);
    }
    let _ = execute!(output, Show, LeaveAlternateScreen);
    let _ = disable_raw_mode();
}
