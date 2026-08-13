use std::{fs, path::Path, process::Command};

use argos_explorer::{
    git::{ChangeGroup, ChangeKind, GitRepo},
    viewer::load_page,
};
use tempfile::TempDir;

fn git(directory: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        .output()
        .expect("Git must be installed for integration tests")
}

fn assert_git(directory: &Path, args: &[&str]) {
    let output = git(directory, args);
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repository() -> TempDir {
    let temp = tempfile::tempdir().unwrap();
    assert_git(temp.path(), &["init", "-b", "main"]);
    assert_git(temp.path(), &["config", "user.name", "Argos Explorer Test"]);
    assert_git(
        temp.path(),
        &["config", "user.email", "argos-explorer@example.invalid"],
    );
    fs::write(temp.path().join("tracked.txt"), "base\n").unwrap();
    assert_git(temp.path(), &["add", "tracked.txt"]);
    assert_git(temp.path(), &["commit", "-m", "base"]);
    temp
}

#[test]
fn reports_staged_unstaged_and_untracked_states_separately() {
    let temp = repository();
    fs::write(temp.path().join("tracked.txt"), "staged\n").unwrap();
    assert_git(temp.path(), &["add", "tracked.txt"]);
    fs::write(temp.path().join("tracked.txt"), "staged\nunstaged\n").unwrap();
    fs::write(temp.path().join("new.txt"), "new\n").unwrap();

    let root = fs::canonicalize(temp.path()).unwrap();
    let repo = GitRepo::discover(&root).unwrap().unwrap();
    let status = repo.status().unwrap();

    assert!(status.iter().any(|entry| {
        entry.path == Path::new("tracked.txt") && entry.group == ChangeGroup::Staged
    }));
    assert!(status.iter().any(|entry| {
        entry.path == Path::new("tracked.txt") && entry.group == ChangeGroup::Unstaged
    }));
    assert!(status.iter().any(|entry| {
        entry.path == Path::new("new.txt") && entry.group == ChangeGroup::Untracked
    }));
}

#[test]
fn staged_rename_retains_old_and_new_paths() {
    let temp = repository();
    assert_git(temp.path(), &["mv", "tracked.txt", "renamed.txt"]);

    let root = fs::canonicalize(temp.path()).unwrap();
    let repo = GitRepo::discover(&root).unwrap().unwrap();
    let status = repo.status().unwrap();
    let renamed = status
        .iter()
        .find(|entry| entry.kind == ChangeKind::Renamed)
        .expect("rename entry");

    assert_eq!(renamed.path, Path::new("renamed.txt"));
    assert_eq!(renamed.old_path.as_deref(), Some(Path::new("tracked.txt")));
}

#[test]
fn untracked_binary_diff_is_metadata_not_terminal_bytes() {
    let temp = repository();
    fs::write(temp.path().join("binary.dat"), [0, 1, 0, 2, 0, 3]).unwrap();

    let root = fs::canonicalize(temp.path()).unwrap();
    let repo = GitRepo::discover(&root).unwrap().unwrap();
    let entry = repo
        .status()
        .unwrap()
        .into_iter()
        .find(|entry| entry.path == Path::new("binary.dat"))
        .unwrap();
    let diff = repo.diff(&entry, 8 * 1024 * 1024).unwrap();

    assert!(diff.binary);
    assert!(diff.text.contains("Binary content differs"));
    assert!(!diff.text.contains('\0'));
}

#[test]
fn large_untracked_text_diff_is_paged() {
    let temp = repository();
    let path = temp.path().join("large.txt");
    fs::write(&path, "scalable line\n".repeat(10_000)).unwrap();

    let root = fs::canonicalize(temp.path()).unwrap();
    let repo = GitRepo::discover(&root).unwrap().unwrap();
    let entry = repo
        .status()
        .unwrap()
        .into_iter()
        .find(|entry| entry.path == Path::new("large.txt"))
        .unwrap();
    let diff = repo.diff(&entry, 1024).unwrap();
    let document = diff.large_untracked.expect("large diff descriptor");
    let page = load_page(&document, 0, 4096).unwrap();

    assert!(!diff.binary);
    assert!(page.text.starts_with("scalable line"));
    assert!(page.next_offset < document.size);
}

#[test]
fn merge_conflict_is_grouped_as_conflict() {
    let temp = repository();
    assert_git(temp.path(), &["checkout", "-b", "side"]);
    fs::write(temp.path().join("tracked.txt"), "side\n").unwrap();
    assert_git(temp.path(), &["commit", "-am", "side"]);
    assert_git(temp.path(), &["checkout", "main"]);
    fs::write(temp.path().join("tracked.txt"), "main\n").unwrap();
    assert_git(temp.path(), &["commit", "-am", "main"]);
    let merge = git(temp.path(), &["merge", "side"]);
    assert!(!merge.status.success(), "merge should conflict");

    let root = fs::canonicalize(temp.path()).unwrap();
    let repo = GitRepo::discover(&root).unwrap().unwrap();
    let status = repo.status().unwrap();

    assert!(status.iter().any(|entry| {
        entry.path == Path::new("tracked.txt") && entry.group == ChangeGroup::Conflict
    }));
}
