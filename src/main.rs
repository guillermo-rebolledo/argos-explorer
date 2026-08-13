use std::{error::Error, process::ExitCode};

use argos_explorer::{app::App, config::Config, diagnostics, terminal::TerminalSession};

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("argos-explorer: {error}");
            let mut source = error.source();
            while let Some(cause) = source {
                eprintln!("  caused by: {cause}");
                source = cause.source();
            }
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let config = Config::load()?;
    let log_path = diagnostics::initialize(config.log_level.as_deref())?;
    if let Some(path) = log_path {
        tracing::info!(path = %path.display(), "diagnostic logging enabled");
    }

    let mut session = TerminalSession::enter(config.mouse)?;
    let mut app = App::new(config);
    app.run(session.terminal_mut())?;
    Ok(())
}
