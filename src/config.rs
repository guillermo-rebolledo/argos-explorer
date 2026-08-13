use std::{env, fs, path::PathBuf};

use clap::{Parser, ValueEnum};
use directories::BaseDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

const DEFAULT_SMALL_FILE_LIMIT: u64 = 8 * 1024 * 1024;
const DEFAULT_PAGE_CACHE: usize = 64 * 1024 * 1024;
const DEFAULT_INDEX_MEMORY: usize = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum IconMode {
    #[default]
    NerdFont,
    Emoji,
    None,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeMode {
    #[default]
    Ansi,
    HighContrast,
    Monochrome,
}

#[derive(Debug, Parser)]
#[command(
    name = "argos-explorer",
    version,
    about = "A scalable Windows-first workspace inspector"
)]
struct Cli {
    /// Workspace root. Defaults to the current directory.
    #[arg(value_name = "DIRECTORY")]
    directory: Option<PathBuf>,

    /// Read configuration from this TOML file.
    #[arg(long, env = "ARGOS_EXPLORER_CONFIG", value_name = "FILE")]
    config: Option<PathBuf>,

    /// File and folder icon vocabulary.
    #[arg(long, env = "ARGOS_EXPLORER_ICONS", value_enum)]
    icons: Option<IconMode>,

    /// Disable Unicode box-drawing characters.
    #[arg(long)]
    ascii: bool,

    /// Disable all color output.
    #[arg(long)]
    no_color: bool,

    /// Disable mouse capture and mouse navigation.
    #[arg(long)]
    no_mouse: bool,

    /// Enable diagnostic logging at this tracing level.
    #[arg(long, env = "ARGOS_EXPLORER_LOG_LEVEL", value_name = "FILTER")]
    log_level: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    icons: Option<IconMode>,
    theme: Option<ThemeMode>,
    ascii: Option<bool>,
    color: Option<bool>,
    mouse: Option<bool>,
    tab_width: Option<u8>,
    small_file_limit_mib: Option<u64>,
    page_cache_mib: Option<usize>,
    index_memory_mib: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub root: PathBuf,
    pub config_path: PathBuf,
    pub icons: IconMode,
    pub theme: ThemeMode,
    pub ascii: bool,
    pub color: bool,
    pub mouse: bool,
    pub tab_width: u8,
    pub small_file_limit: u64,
    pub page_cache_bytes: usize,
    pub index_memory_bytes: usize,
    pub log_level: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("the Workspace Root does not exist or is inaccessible: {path}: {source}")]
    InaccessibleRoot {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("the Workspace Root is not a directory: {0}")]
    RootNotDirectory(PathBuf),
    #[error("could not read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("could not determine the current directory: {0}")]
    CurrentDirectory(#[source] std::io::Error),
}

impl Config {
    pub fn load() -> Result<Self, ConfigError> {
        let cli = Cli::parse();
        Self::from_cli(cli)
    }

    fn from_cli(cli: Cli) -> Result<Self, ConfigError> {
        let config_path = cli.config.unwrap_or_else(default_config_path);
        let file = if config_path.exists() {
            let source = fs::read_to_string(&config_path).map_err(|source| ConfigError::Read {
                path: config_path.clone(),
                source,
            })?;
            toml::from_str(&source).map_err(|source| ConfigError::Parse {
                path: config_path.clone(),
                source,
            })?
        } else {
            FileConfig::default()
        };

        let requested_root = match cli.directory {
            Some(path) => path,
            None => env::current_dir().map_err(ConfigError::CurrentDirectory)?,
        };
        let root =
            fs::canonicalize(&requested_root).map_err(|source| ConfigError::InaccessibleRoot {
                path: requested_root.clone(),
                source,
            })?;
        if !root.is_dir() {
            return Err(ConfigError::RootNotDirectory(root));
        }

        let no_color_env = env::var_os("NO_COLOR").is_some();
        Ok(Self {
            root,
            config_path,
            icons: cli.icons.or(file.icons).unwrap_or_default(),
            theme: file.theme.unwrap_or_default(),
            ascii: cli.ascii || file.ascii.unwrap_or(false),
            color: !cli.no_color && !no_color_env && file.color.unwrap_or(true),
            mouse: !cli.no_mouse && file.mouse.unwrap_or(true),
            tab_width: file.tab_width.unwrap_or(4).clamp(1, 16),
            small_file_limit: file
                .small_file_limit_mib
                .map(|value| value.saturating_mul(1024 * 1024))
                .unwrap_or(DEFAULT_SMALL_FILE_LIMIT),
            page_cache_bytes: file
                .page_cache_mib
                .map(|value| value.saturating_mul(1024 * 1024))
                .unwrap_or(DEFAULT_PAGE_CACHE),
            index_memory_bytes: file
                .index_memory_mib
                .map(|value| value.saturating_mul(1024 * 1024))
                .unwrap_or(DEFAULT_INDEX_MEMORY),
            log_level: cli.log_level,
        })
    }

    pub fn display_root(&self) -> String {
        display_path(&self.root)
    }
}

pub fn display_path(path: &std::path::Path) -> String {
    let value = path.to_string_lossy();
    #[cfg(windows)]
    {
        if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
            return format!("\\\\{rest}");
        }
        if let Some(rest) = value.strip_prefix("\\\\?\\") {
            return rest.to_owned();
        }
    }
    value.into_owned()
}

pub fn default_config_path() -> PathBuf {
    BaseDirs::new()
        .map(|dirs| dirs.config_dir().join("argos-explorer").join("config.toml"))
        .unwrap_or_else(|| PathBuf::from("config.toml"))
}

pub fn default_log_dir() -> PathBuf {
    BaseDirs::new()
        .map(|dirs| dirs.data_local_dir().join("argos-explorer").join("logs"))
        .unwrap_or_else(|| PathBuf::from("logs"))
}
