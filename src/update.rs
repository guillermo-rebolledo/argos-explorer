use std::{
    env,
    fs::File,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use reqwest::{StatusCode, blocking::Client, header};
use semver::Version;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::config::UpdateOptions;

const RELEASES_API: &str =
    "https://api.github.com/repos/guillermo-rebolledo/argos-explorer/releases";
const SETUP_ASSET: &str = "argos-explorer-setup.exe";
const CHECKSUM_ASSET: &str = "argos-explorer-setup.exe.sha256";
const MAX_INSTALLER_BYTES: u64 = 100 * 1024 * 1024;
const MAX_CHECKSUM_BYTES: u64 = 64 * 1024;
const PRODUCT_DIRECTORY: &str = "argos-explorer";

pub const BUILD_TAG: &str = env!("ARGOS_EXPLORER_BUILD_TAG");
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum UpdateChannel {
    Stable,
    Preview,
}

#[derive(Debug, Clone, Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<ReleaseAsset>,
}

impl Release {
    fn asset(&self, name: &str) -> Option<&ReleaseAsset> {
        self.assets.iter().find(|asset| asset.name == name)
    }

    fn version(&self, channel: UpdateChannel) -> Option<Version> {
        match channel {
            UpdateChannel::Stable => Version::parse(self.tag_name.strip_prefix('v')?).ok(),
            UpdateChannel::Preview => {
                let value = self.tag_name.strip_prefix("preview-v")?;
                let version = value.split_once("-build-")?.0;
                Version::parse(version).ok()
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}

struct GithubReleaseSource {
    client: Client,
}

impl GithubReleaseSource {
    fn new() -> Result<Self, UpdateError> {
        let client = Client::builder()
            .user_agent(format!("argos-explorer/{VERSION}"))
            .timeout(Duration::from_secs(60))
            .connect_timeout(Duration::from_secs(15))
            .https_only(true)
            .build()?;
        Ok(Self { client })
    }

    fn latest(&self, channel: UpdateChannel) -> Result<Option<Release>, UpdateError> {
        match channel {
            UpdateChannel::Stable => {
                let response = self.request(&format!("{RELEASES_API}/latest"))?.send()?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                let release = response.error_for_status()?.json::<Release>()?;
                Ok((!release.draft && !release.prerelease).then_some(release))
            }
            UpdateChannel::Preview => {
                let releases = self
                    .request(&format!("{RELEASES_API}?per_page=30"))?
                    .send()?
                    .error_for_status()?
                    .json::<Vec<Release>>()?;
                Ok(releases.into_iter().find(|release| {
                    !release.draft
                        && release.prerelease
                        && release.version(UpdateChannel::Preview).is_some()
                }))
            }
        }
    }

    fn download(
        &self,
        asset: &ReleaseAsset,
        destination: &Path,
        maximum: u64,
    ) -> Result<(), UpdateError> {
        if !asset.browser_download_url.starts_with("https://") {
            return Err(UpdateError::InsecureDownload(
                asset.browser_download_url.clone(),
            ));
        }
        let response = self
            .request(&asset.browser_download_url)?
            .send()?
            .error_for_status()?;
        if response
            .content_length()
            .is_some_and(|length| length > maximum)
        {
            return Err(UpdateError::DownloadTooLarge {
                name: asset.name.clone(),
                maximum,
            });
        }
        let mut file = File::create(destination)?;
        let mut limited = response.take(maximum + 1);
        let copied = io::copy(&mut limited, &mut file)?;
        file.flush()?;
        if copied > maximum {
            return Err(UpdateError::DownloadTooLarge {
                name: asset.name.clone(),
                maximum,
            });
        }
        Ok(())
    }

    fn request(&self, url: &str) -> Result<reqwest::blocking::RequestBuilder, UpdateError> {
        if !url.starts_with("https://") {
            return Err(UpdateError::InsecureDownload(url.to_owned()));
        }
        Ok(self
            .client
            .get(url)
            .header(header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28"))
    }
}

#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("GitHub request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("update file operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("release {tag} does not contain {asset}")]
    MissingAsset { tag: String, asset: &'static str },
    #[error("release {0} does not contain a valid channel version")]
    InvalidVersion(String),
    #[error("refusing insecure update download URL: {0}")]
    InsecureDownload(String),
    #[error("download {name} exceeds the {maximum}-byte safety limit")]
    DownloadTooLarge { name: String, maximum: u64 },
    #[error("invalid installer checksum file")]
    InvalidChecksum,
    #[error("installer checksum mismatch: expected {expected}, calculated {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("self-update is currently supported only on Windows")]
    UnsupportedPlatform,
}

pub fn run(options: UpdateOptions) -> Result<(), UpdateError> {
    let channel = if options.preview {
        UpdateChannel::Preview
    } else {
        UpdateChannel::Stable
    };
    let source = GithubReleaseSource::new()?;
    let Some(release) = source.latest(channel)? else {
        println!(
            "No {} release is currently published.",
            channel_name(channel)
        );
        if channel == UpdateChannel::Stable {
            println!("Use `argos-explorer update --preview` to check merged-PR preview builds.");
        }
        return Ok(());
    };
    let release_version = release
        .version(channel)
        .ok_or_else(|| UpdateError::InvalidVersion(release.tag_name.clone()))?;
    let current_version = Version::parse(VERSION)
        .map_err(|_| UpdateError::InvalidVersion(format!("current version {VERSION}")))?;

    println!("Current:   {VERSION} ({BUILD_TAG})");
    println!("Available: {} ({})", release_version, release.tag_name);
    if !update_is_needed(&release, channel, &current_version, BUILD_TAG) {
        println!(
            "argos-explorer is already up to date on the {} channel.",
            channel_name(channel)
        );
        return Ok(());
    }
    if options.check {
        println!("An update is available: {}", release.html_url);
        return Ok(());
    }

    let setup_asset = release
        .asset(SETUP_ASSET)
        .ok_or_else(|| UpdateError::MissingAsset {
            tag: release.tag_name.clone(),
            asset: SETUP_ASSET,
        })?;
    let checksum_asset =
        release
            .asset(CHECKSUM_ASSET)
            .ok_or_else(|| UpdateError::MissingAsset {
                tag: release.tag_name.clone(),
                asset: CHECKSUM_ASSET,
            })?;

    println!("Downloading verified update from {}", release.html_url);
    let temporary = tempfile::Builder::new()
        .prefix("argos-explorer-update-")
        .tempdir()?;
    let setup_path = temporary.path().join(SETUP_ASSET);
    let checksum_path = temporary.path().join(CHECKSUM_ASSET);
    source.download(checksum_asset, &checksum_path, MAX_CHECKSUM_BYTES)?;
    source.download(setup_asset, &setup_path, MAX_INSTALLER_BYTES)?;
    verify_checksum(&setup_path, &checksum_path)?;

    let cleanup_dir = temporary.keep();
    if let Err(error) = stage_update(&setup_path, &cleanup_dir) {
        let _ = std::fs::remove_dir_all(&cleanup_dir);
        return Err(error);
    }
    println!("Update verified and staged. argos-explorer will exit so Windows can replace it.");
    Ok(())
}

fn update_is_needed(
    release: &Release,
    channel: UpdateChannel,
    current_version: &Version,
    current_build_tag: &str,
) -> bool {
    if release.tag_name == current_build_tag {
        return false;
    }
    let Some(release_version) = release.version(channel) else {
        return false;
    };
    match release_version.cmp(current_version) {
        std::cmp::Ordering::Greater => true,
        std::cmp::Ordering::Less => false,
        std::cmp::Ordering::Equal => match channel {
            UpdateChannel::Preview => true,
            UpdateChannel::Stable => {
                current_build_tag.starts_with("preview-") || current_build_tag.starts_with("build-")
            }
        },
    }
}

fn verify_checksum(executable: &Path, checksum_file: &Path) -> Result<(), UpdateError> {
    let checksum = std::fs::read_to_string(checksum_file)?;
    let expected = parse_checksum(&checksum).ok_or(UpdateError::InvalidChecksum)?;
    let mut file = File::open(executable)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    let actual = lowercase_hex(&hasher.finalize());
    if actual.eq_ignore_ascii_case(&expected) {
        Ok(())
    } else {
        Err(UpdateError::ChecksumMismatch { expected, actual })
    }
}

fn parse_checksum(content: &str) -> Option<String> {
    let checksum = content.split_whitespace().next()?;
    (checksum.len() == 64 && checksum.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| checksum.to_ascii_lowercase())
}

fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(windows)]
fn stage_update(setup_path: &Path, cleanup_dir: &Path) -> Result<(), UpdateError> {
    let current_executable = std::fs::canonicalize(env::current_exe()?)?;
    let managed = default_install_dir().is_some_and(|directory| {
        current_executable
            .parent()
            .is_some_and(|parent| same_path(parent, &directory))
    });
    let mut command = Command::new(setup_path);
    command
        .arg("--quiet")
        .args(["--internal-wait-pid", &std::process::id().to_string()])
        .arg("--internal-cleanup-dir")
        .arg(cleanup_dir)
        .stdin(Stdio::null());
    if !managed {
        command
            .arg("--internal-portable-target")
            .arg(&current_executable);
    }
    command.spawn()?;
    Ok(())
}

#[cfg(not(windows))]
fn stage_update(_setup_path: &Path, _cleanup_dir: &Path) -> Result<(), UpdateError> {
    Err(UpdateError::UnsupportedPlatform)
}

#[cfg(windows)]
fn default_install_dir() -> Option<PathBuf> {
    env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|directory| directory.join("Programs").join(PRODUCT_DIRECTORY))
}

#[cfg(windows)]
fn same_path(left: &Path, right: &Path) -> bool {
    normalize_path(left) == normalize_path(right)
}

#[cfg(windows)]
fn normalize_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    let value = if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        rest.to_owned()
    } else {
        value
    };
    value.trim_end_matches('\\').to_lowercase()
}

fn channel_name(channel: UpdateChannel) -> &'static str {
    match channel {
        UpdateChannel::Stable => "stable",
        UpdateChannel::Preview => "preview",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, prerelease: bool) -> Release {
        Release {
            tag_name: tag.to_owned(),
            html_url: "https://example.invalid/release".to_owned(),
            draft: false,
            prerelease,
            assets: Vec::new(),
        }
    }

    #[test]
    fn parses_stable_and_preview_versions() {
        assert_eq!(
            release("v1.2.3", false).version(UpdateChannel::Stable),
            Some(Version::new(1, 2, 3))
        );
        assert_eq!(
            release("preview-v1.2.3-build-42-abcdef0", true).version(UpdateChannel::Preview),
            Some(Version::new(1, 2, 3))
        );
        assert!(
            release("build-3-abcdef0", true)
                .version(UpdateChannel::Preview)
                .is_none()
        );
    }

    #[test]
    fn parses_binary_sha256_lines() {
        let hash = "a".repeat(64);
        assert_eq!(
            parse_checksum(&format!("{hash} *argos-explorer-setup.exe")),
            Some(hash)
        );
        assert!(parse_checksum("not-a-checksum").is_none());
    }

    #[test]
    fn verifies_matching_and_rejects_mismatched_checksums() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("setup.exe");
        let checksum = temp.path().join("setup.sha256");
        std::fs::write(&executable, b"trusted update bytes").unwrap();
        let expected = lowercase_hex(&Sha256::digest(b"trusted update bytes"));
        std::fs::write(&checksum, format!("{expected} *setup.exe")).unwrap();
        verify_checksum(&executable, &checksum).unwrap();

        std::fs::write(&checksum, format!("{} *setup.exe", "0".repeat(64))).unwrap();
        assert!(matches!(
            verify_checksum(&executable, &checksum),
            Err(UpdateError::ChecksumMismatch { .. })
        ));
    }

    #[test]
    fn compares_stable_and_preview_channels() {
        let current = Version::new(1, 2, 3);
        assert!(!update_is_needed(
            &release("v1.2.3", false),
            UpdateChannel::Stable,
            &current,
            "v1.2.3"
        ));
        assert!(update_is_needed(
            &release("v1.2.4", false),
            UpdateChannel::Stable,
            &current,
            "v1.2.3"
        ));
        assert!(update_is_needed(
            &release("v1.2.3", false),
            UpdateChannel::Stable,
            &current,
            "preview-v1.2.3-build-41-aaaaaaa"
        ));
        assert!(update_is_needed(
            &release("preview-v1.2.3-build-42-bbbbbbb", true),
            UpdateChannel::Preview,
            &current,
            "preview-v1.2.3-build-41-aaaaaaa"
        ));
        assert!(!update_is_needed(
            &release("preview-v1.2.2-build-42-bbbbbbb", true),
            UpdateChannel::Preview,
            &current,
            "v1.2.3"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn extended_windows_paths_match_normal_install_paths() {
        assert!(same_path(
            Path::new(r"\\?\C:\Users\User\AppData\Local\Programs\argos-explorer"),
            Path::new(r"C:\Users\User\AppData\Local\Programs\argos-explorer")
        ));
    }
}
