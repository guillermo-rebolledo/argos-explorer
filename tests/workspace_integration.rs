use std::{fs, path::Path};

use argos_explorer::workspace::{EntryKind, TreeAction, WorkspaceTree, load_directory};

#[test]
fn directory_listing_is_directory_first_and_naturally_sorted() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join("folder")).unwrap();
    fs::write(temp.path().join("file10.txt"), "ten").unwrap();
    fs::write(temp.path().join("file2.txt"), "two").unwrap();

    let root = fs::canonicalize(temp.path()).unwrap();
    let listing = load_directory(&root, &root, false);
    let names: Vec<_> = listing
        .entries
        .iter()
        .map(|entry| entry.name.to_string_lossy().into_owned())
        .collect();

    assert_eq!(names, ["folder", "file2.txt", "file10.txt"]);
}

#[test]
fn dotfiles_are_visible_and_marked_hidden() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join(".secret"), "visible").unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let listing = load_directory(&root, &root, false);
    let entry = listing
        .entries
        .iter()
        .find(|entry| entry.name == ".secret")
        .unwrap();
    assert!(entry.hidden);
}

#[test]
fn internal_git_directory_is_hidden_until_requested() {
    let temp = tempfile::tempdir().unwrap();
    fs::create_dir(temp.path().join(".git")).unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();

    assert!(
        load_directory(&root, &root, false)
            .entries
            .iter()
            .all(|entry| entry.name != ".git")
    );
    assert!(
        load_directory(&root, &root, true)
            .entries
            .iter()
            .any(|entry| entry.name == ".git")
    );
}

#[test]
fn ordinary_file_activation_opens_preview() {
    let temp = tempfile::tempdir().unwrap();
    fs::write(temp.path().join("file.txt"), "content").unwrap();
    let root = fs::canonicalize(temp.path()).unwrap();
    let listing = load_directory(&root, &root, false);
    let mut tree = WorkspaceTree::new(root.clone());
    tree.apply_listing(listing);
    assert!(tree.select_visible(0, ""));

    assert_eq!(
        tree.activate_selected(),
        TreeAction::OpenFile(root.join("file.txt"))
    );
    assert_eq!(tree.selected_node().unwrap().kind, EntryKind::File);
}

#[cfg(windows)]
#[test]
fn external_file_symlink_is_never_opened() {
    use std::os::windows::fs::symlink_file;

    let workspace = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let link = workspace.path().join("external.txt");
    if symlink_file(outside.path(), &link).is_err() {
        return; // Windows developer mode or symlink privilege is unavailable.
    }
    let root = fs::canonicalize(workspace.path()).unwrap();
    let listing = load_directory(&root, &root, false);
    let mut tree = WorkspaceTree::new(root);
    tree.apply_listing(listing);
    assert!(tree.select_visible(0, ""));

    assert!(matches!(tree.activate_selected(), TreeAction::Message(_)));
}

#[allow(dead_code)]
fn _path_type_check(_: &Path) {}
