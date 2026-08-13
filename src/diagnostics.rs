use std::{
    fs::{self, File},
    io,
    path::PathBuf,
    sync::Mutex,
};

use thiserror::Error;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

use crate::config::default_log_dir;

#[derive(Debug, Error)]
pub enum DiagnosticsError {
    #[error("could not create diagnostic log directory {path}: {source}")]
    CreateDirectory {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("could not create diagnostic log {path}: {source}")]
    CreateLog {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("invalid diagnostic filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    #[error("could not initialize diagnostics: {0}")]
    Initialize(String),
}

pub fn initialize(level: Option<&str>) -> Result<Option<PathBuf>, DiagnosticsError> {
    let Some(level) = level else {
        return Ok(None);
    };

    let directory = default_log_dir();
    fs::create_dir_all(&directory).map_err(|source| DiagnosticsError::CreateDirectory {
        path: directory.clone(),
        source,
    })?;
    let path = directory.join("argos-explorer.log");
    let file = File::create(&path).map_err(|source| DiagnosticsError::CreateLog {
        path: path.clone(),
        source,
    })?;
    let filter = EnvFilter::try_new(level)?;
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_ansi(false).with_writer(Mutex::new(file)))
        .try_init()
        .map_err(|error| DiagnosticsError::Initialize(error.to_string()))?;
    Ok(Some(path))
}
