use std::{
    collections::HashSet,
    ffi::OsStr,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

use thiserror::Error;

use crate::{
    viewer::{LargeDocument, LoadedDocument},
    workspace::{DirectoryListing, path_is_within},
};

#[derive(Debug, Clone)]
pub struct GitRepo {
    pub root: PathBuf,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ChangeGroup {
    Conflict,
    Staged,
    Unstaged,
    Untracked,
}

impl ChangeGroup {
    pub fn label(self) -> &'static str {
        match self {
            Self::Conflict => "Conflicts",
            Self::Staged => "Staged Changes",
            Self::Unstaged => "Unstaged Changes",
            Self::Untracked => "Untracked Files",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    TypeChanged,
    Unmerged,
    Untracked,
    Unknown,
}

impl ChangeKind {
    pub fn marker(self) -> &'static str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
            Self::TypeChanged => "T",
            Self::Unmerged => "U",
            Self::Untracked => "?",
            Self::Unknown => "!",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangeEntry {
    pub path: PathBuf,
    pub old_path: Option<PathBuf>,
    pub group: ChangeGroup,
    pub kind: ChangeKind,
    pub xy: [char; 2],
}

impl ChangeEntry {
    pub fn display_path(&self) -> String {
        self.path.to_string_lossy().replace('\\', "/")
    }
}

#[derive(Debug, Clone)]
pub struct DiffOutput {
    pub title: String,
    pub text: String,
    pub binary: bool,
    pub large_untracked: Option<LargeDocument>,
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("Git is not installed or is not available on PATH")]
    Unavailable,
    #[error("Git command failed: {0}")]
    Command(String),
    #[error("could not start Git: {0}")]
    Start(#[source] std::io::Error),
    #[error("could not write to Git: {0}")]
    Input(#[source] std::io::Error),
    #[error("could not read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("could not inspect diff content: {0}")]
    Inspect(String),
}

impl GitRepo {
    pub fn discover(workspace: &Path) -> Result<Option<Self>, GitError> {
        let output = Command::new("git")
            .arg("-C")
            .arg(workspace)
            .args(["rev-parse", "--show-toplevel"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .map_err(map_start_error)?;
        if !output.status.success() {
            return Ok(None);
        }
        let root_text = String::from_utf8_lossy(&output.stdout);
        let parsed_root = PathBuf::from(root_text.trim_end_matches(['\r', '\n']));
        let root = fs::canonicalize(&parsed_root).unwrap_or(parsed_root);
        Ok(Some(Self {
            root,
            workspace: workspace.to_path_buf(),
        }))
    }

    pub fn status(&self) -> Result<Vec<ChangeEntry>, GitError> {
        let output = self.run([
            OsStr::new("status"),
            OsStr::new("--porcelain=v2"),
            OsStr::new("-z"),
            OsStr::new("--untracked-files=all"),
        ])?;
        if !output.status.success() {
            return Err(GitError::Command(stderr_message(&output.stderr)));
        }
        let mut entries = parse_porcelain_v2(&output.stdout);
        entries.retain(|entry| {
            let full = self.root.join(&entry.path);
            path_is_within(&self.workspace, &full)
        });
        for entry in &mut entries {
            entry.path = self
                .root
                .join(&entry.path)
                .strip_prefix(&self.workspace)
                .unwrap_or(&entry.path)
                .to_path_buf();
            if let Some(old) = &entry.old_path {
                let old_full = self.root.join(old);
                entry.old_path = Some(
                    old_full
                        .strip_prefix(&self.workspace)
                        .unwrap_or(old)
                        .to_path_buf(),
                );
            }
        }
        entries.sort_by(|left, right| {
            left.group.cmp(&right.group).then_with(|| {
                left.display_path()
                    .to_lowercase()
                    .cmp(&right.display_path().to_lowercase())
            })
        });
        Ok(entries)
    }

    pub fn diff(&self, entry: &ChangeEntry, small_file_limit: u64) -> Result<DiffOutput, GitError> {
        let full_path = self.workspace.join(&entry.path);
        if entry.group == ChangeGroup::Untracked {
            return untracked_diff(&full_path, entry, small_file_limit);
        }

        let relative_to_repo = full_path.strip_prefix(&self.root).unwrap_or(&entry.path);
        let mut command = Command::new("git");
        command.arg("-C").arg(&self.root).args([
            "-c",
            "core.quotepath=false",
            "diff",
            "--no-ext-diff",
            "--no-color",
            "--unified=3",
        ]);
        if entry.group == ChangeGroup::Staged {
            command.arg("--cached");
        }
        if entry.group == ChangeGroup::Conflict {
            command.arg("--cc");
        }
        command.arg("--").arg(relative_to_repo);
        let output = command
            .stdin(Stdio::null())
            .output()
            .map_err(map_start_error)?;
        if !output.status.success() {
            return Err(GitError::Command(stderr_message(&output.stderr)));
        }
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        if entry.group == ChangeGroup::Conflict && text.trim().is_empty() {
            text = fs::read_to_string(&full_path).map_err(|source| GitError::Read {
                path: full_path.clone(),
                source,
            })?;
        }
        let binary = text.contains("Binary files") || text.contains("GIT binary patch");
        let title = match &entry.old_path {
            Some(old) => format!("{} → {}", old.display(), entry.path.display()),
            None => entry.path.display().to_string(),
        };
        Ok(DiffOutput {
            title,
            text,
            binary,
            large_untracked: None,
        })
    }

    pub fn seed_files(&self) -> Result<Vec<PathBuf>, GitError> {
        let output = self.run([
            OsStr::new("ls-files"),
            OsStr::new("-z"),
            OsStr::new("--cached"),
            OsStr::new("--others"),
            OsStr::new("--exclude-standard"),
        ])?;
        if !output.status.success() {
            return Err(GitError::Command(stderr_message(&output.stderr)));
        }
        Ok(output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
            .map(|record| self.root.join(String::from_utf8_lossy(record).as_ref()))
            .filter(|path| path_is_within(&self.workspace, path))
            .collect())
    }

    pub fn mark_ignored(&self, listing: &mut DirectoryListing) -> Result<(), GitError> {
        let mut command = Command::new("git");
        command
            .arg("-C")
            .arg(&self.root)
            .args(["check-ignore", "-z", "--stdin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = command.spawn().map_err(map_start_error)?;
        if let Some(mut stdin) = child.stdin.take() {
            for entry in &listing.entries {
                let relative = entry.path.strip_prefix(&self.root).unwrap_or(&entry.path);
                stdin
                    .write_all(relative.to_string_lossy().as_bytes())
                    .and_then(|_| stdin.write_all(&[0]))
                    .map_err(GitError::Input)?;
            }
        }
        let output = child.wait_with_output().map_err(GitError::Start)?;
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(GitError::Command(stderr_message(&output.stderr)));
        }
        let ignored: HashSet<_> = output
            .stdout
            .split(|byte| *byte == 0)
            .filter(|record| !record.is_empty())
            .map(|record| self.root.join(String::from_utf8_lossy(record).as_ref()))
            .collect();
        for entry in &mut listing.entries {
            entry.ignored = ignored.contains(&entry.path);
        }
        Ok(())
    }

    fn run<I, S>(&self, args: I) -> Result<std::process::Output, GitError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .map_err(map_start_error)
    }
}

fn parse_porcelain_v2(bytes: &[u8]) -> Vec<ChangeEntry> {
    let records: Vec<&[u8]> = bytes.split(|byte| *byte == 0).collect();
    let mut entries = Vec::new();
    let mut index = 0;
    while index < records.len() {
        let record = records[index];
        index += 1;
        if record.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(record);
        if let Some(path) = text.strip_prefix("? ") {
            entries.push(ChangeEntry {
                path: PathBuf::from(path),
                old_path: None,
                group: ChangeGroup::Untracked,
                kind: ChangeKind::Untracked,
                xy: ['?', '?'],
            });
            continue;
        }
        if text.starts_with("! ") || text.starts_with("# ") {
            continue;
        }

        let tag = text.as_bytes()[0];
        let field_count = match tag {
            b'1' => 9,
            b'2' => 10,
            b'u' => 11,
            _ => continue,
        };
        let fields: Vec<&str> = text.splitn(field_count, ' ').collect();
        if fields.len() < field_count {
            continue;
        }
        let xy_text = fields[1];
        let mut xy_chars = xy_text.chars();
        let x = xy_chars.next().unwrap_or('.');
        let y = xy_chars.next().unwrap_or('.');
        let path = PathBuf::from(fields[field_count - 1]);
        let old_path = if tag == b'2' && index < records.len() {
            let old = records[index];
            index += 1;
            Some(PathBuf::from(String::from_utf8_lossy(old).as_ref()))
        } else {
            None
        };

        if tag == b'u' || is_conflict(x, y) {
            entries.push(ChangeEntry {
                path,
                old_path,
                group: ChangeGroup::Conflict,
                kind: ChangeKind::Unmerged,
                xy: [x, y],
            });
            continue;
        }
        if x != '.' {
            entries.push(ChangeEntry {
                path: path.clone(),
                old_path: old_path.clone(),
                group: ChangeGroup::Staged,
                kind: kind_from_status(x),
                xy: [x, y],
            });
        }
        if y != '.' {
            entries.push(ChangeEntry {
                path,
                old_path,
                group: ChangeGroup::Unstaged,
                kind: kind_from_status(y),
                xy: [x, y],
            });
        }
    }
    entries
}

fn is_conflict(x: char, y: char) -> bool {
    x == 'U' || y == 'U' || matches!((x, y), ('A', 'A') | ('D', 'D') | ('A', 'D') | ('D', 'A'))
}

fn kind_from_status(status: char) -> ChangeKind {
    match status {
        'A' => ChangeKind::Added,
        'M' => ChangeKind::Modified,
        'D' => ChangeKind::Deleted,
        'R' => ChangeKind::Renamed,
        'C' => ChangeKind::Copied,
        'T' => ChangeKind::TypeChanged,
        'U' => ChangeKind::Unmerged,
        '?' => ChangeKind::Untracked,
        _ => ChangeKind::Unknown,
    }
}

fn untracked_diff(
    path: &Path,
    entry: &ChangeEntry,
    small_file_limit: u64,
) -> Result<DiffOutput, GitError> {
    let loaded = crate::viewer::load_document(path, small_file_limit)
        .map_err(|error| GitError::Inspect(error.to_string()))?;
    let title = entry.path.display().to_string();
    match loaded {
        LoadedDocument::Binary(document) => Ok(DiffOutput {
            title,
            text: format!(
                "Binary content differs\nPath: {}\nSize: {} bytes\n",
                entry.path.display(),
                document.size
            ),
            binary: true,
            large_untracked: None,
        }),
        LoadedDocument::Text(document) => {
            let mut patch = format!(
                "diff --git a/{0} b/{0}\nnew file mode 100644\n--- /dev/null\n+++ b/{0}\n@@ -0,0 +1,{1} @@\n",
                entry.display_path(),
                document.line_count()
            );
            for line in document.text.lines() {
                patch.push('+');
                patch.push_str(line);
                patch.push('\n');
            }
            Ok(DiffOutput {
                title,
                text: patch,
                binary: false,
                large_untracked: None,
            })
        }
        LoadedDocument::Large(document) => Ok(DiffOutput {
            title,
            text: format!(
                "Large untracked file: {} bytes; content is paged as additions.\n",
                document.size
            ),
            binary: false,
            large_untracked: Some(document),
        }),
    }
}

fn map_start_error(error: std::io::Error) -> GitError {
    if error.kind() == std::io::ErrorKind::NotFound {
        GitError::Unavailable
    } else {
        GitError::Start(error)
    }
}

fn stderr_message(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let trimmed = text.trim();
    if trimmed.is_empty() {
        "unknown Git error".to_owned()
    } else {
        trimmed.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn mixed_status_creates_staged_and_unstaged_entries() {
        let bytes = b"1 MM N... 100644 100644 100644 abc def src/main.rs\0";
        let entries = parse_porcelain_v2(bytes);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].group, ChangeGroup::Staged);
        assert_eq!(entries[1].group, ChangeGroup::Unstaged);
    }

    #[test]
    fn rename_consumes_original_path_record() {
        let bytes = b"2 R. N... 100644 100644 100644 abc def R100 new.rs\0old.rs\0";
        let entries = parse_porcelain_v2(bytes);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].old_path.as_deref(), Some(Path::new("old.rs")));
        assert_eq!(entries[0].path, PathBuf::from("new.rs"));
    }

    proptest! {
        #[test]
        fn porcelain_parser_never_panics_on_arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let _ = parse_porcelain_v2(&bytes);
        }
    }
}
