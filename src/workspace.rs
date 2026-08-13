use std::{
    cmp::Ordering,
    collections::HashMap,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    LinkFile,
    LinkDirectory,
    Other,
    Error,
}

impl EntryKind {
    pub fn is_directory(self) -> bool {
        matches!(self, Self::Directory | Self::LinkDirectory)
    }

    pub fn is_link(self) -> bool {
        matches!(self, Self::LinkFile | Self::LinkDirectory)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadState {
    Unloaded,
    Loading,
    Loaded,
    Failed,
}

#[derive(Debug, Clone)]
pub struct EntryInfo {
    pub path: PathBuf,
    pub name: OsString,
    pub kind: EntryKind,
    pub hidden: bool,
    pub ignored: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DirectoryListing {
    pub path: PathBuf,
    pub entries: Vec<EntryInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub path: PathBuf,
    pub name: OsString,
    pub kind: EntryKind,
    pub hidden: bool,
    pub ignored: bool,
    pub expanded: bool,
    pub load_state: LoadState,
    pub children: Vec<PathBuf>,
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct VisibleEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: EntryKind,
    pub depth: usize,
    pub hidden: bool,
    pub ignored: bool,
    pub expanded: bool,
    pub load_state: LoadState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeAction {
    None,
    LoadDirectory(PathBuf),
    OpenFile(PathBuf),
    Message(String),
}

#[derive(Debug)]
pub struct WorkspaceTree {
    root: PathBuf,
    nodes: HashMap<PathBuf, Node>,
    selected: Option<PathBuf>,
    pub show_git_directory: bool,
}

impl WorkspaceTree {
    pub fn new(root: PathBuf) -> Self {
        let name = root
            .file_name()
            .map(OsStr::to_os_string)
            .unwrap_or_else(|| root.as_os_str().to_os_string());
        let root_node = Node {
            path: root.clone(),
            name,
            kind: EntryKind::Directory,
            hidden: false,
            ignored: false,
            expanded: true,
            load_state: LoadState::Loading,
            children: Vec::new(),
            error: None,
        };
        let mut nodes = HashMap::new();
        nodes.insert(root.clone(), root_node);
        Self {
            root,
            nodes,
            selected: None,
            show_git_directory: false,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn has_node(&self, path: &Path) -> bool {
        self.nodes.contains_key(path)
    }

    pub fn expanded_directories(&self) -> Vec<PathBuf> {
        self.nodes
            .values()
            .filter(|node| node.kind.is_directory() && node.expanded)
            .map(|node| node.path.clone())
            .collect()
    }

    pub fn selected_path(&self) -> Option<&Path> {
        self.selected.as_deref()
    }

    pub fn selected_node(&self) -> Option<&Node> {
        self.selected.as_ref().and_then(|path| self.nodes.get(path))
    }

    pub fn mark_loading(&mut self, path: &Path) {
        if let Some(node) = self.nodes.get_mut(path) {
            node.load_state = LoadState::Loading;
            node.error = None;
        }
    }

    pub fn apply_listing(&mut self, listing: DirectoryListing) {
        let Some(parent) = self.nodes.get_mut(&listing.path) else {
            return;
        };
        if let Some(error) = listing.error {
            parent.load_state = LoadState::Failed;
            parent.error = Some(error);
            return;
        }

        let old_children = std::mem::take(&mut parent.children);
        parent.children = listing
            .entries
            .iter()
            .map(|entry| entry.path.clone())
            .collect();
        parent.load_state = LoadState::Loaded;
        parent.expanded = true;
        parent.error = None;

        for entry in listing.entries {
            let existing = self.nodes.remove(&entry.path);
            let (expanded, load_state, children) = existing
                .map(|node| (node.expanded, node.load_state, node.children))
                .unwrap_or((false, LoadState::Unloaded, Vec::new()));
            self.nodes.insert(
                entry.path.clone(),
                Node {
                    path: entry.path,
                    name: entry.name,
                    kind: entry.kind,
                    hidden: entry.hidden,
                    ignored: entry.ignored,
                    expanded,
                    load_state,
                    children,
                    error: entry.error,
                },
            );
        }

        for old in old_children {
            if !self
                .nodes
                .get(&listing.path)
                .is_some_and(|node| node.children.contains(&old))
            {
                self.remove_subtree(&old);
            }
        }

        if self.selected.is_none() {
            self.selected = self
                .nodes
                .get(&listing.path)
                .and_then(|node| node.children.first().cloned());
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|path| !self.nodes.contains_key(path))
        {
            self.selected = self
                .nodes
                .get(&listing.path)
                .and_then(|node| node.children.first().cloned())
                .or(Some(listing.path));
        }
    }

    pub fn visible(&self, query: &str) -> Vec<VisibleEntry> {
        let mut rows = Vec::new();
        if query.is_empty() {
            self.collect_visible(&self.root, 0, &mut rows);
        } else {
            let mut matches: Vec<_> = self
                .nodes
                .values()
                .filter(|node| node.path != self.root)
                .filter_map(|node| {
                    let candidate = node
                        .path
                        .strip_prefix(&self.root)
                        .unwrap_or(&node.path)
                        .to_string_lossy();
                    crate::search::fuzzy_score(&candidate, query).map(|score| {
                        (
                            score,
                            self.visible_node(node, self.depth(node.path.as_path())),
                        )
                    })
                })
                .collect();
            matches.sort_by(|(left_score, left), (right_score, right)| {
                left_score
                    .cmp(right_score)
                    .then_with(|| natural_cmp(&left.name, &right.name))
            });
            rows.extend(matches.into_iter().map(|(_, row)| row));
        }
        rows
    }

    pub fn move_selection(&mut self, delta: isize, query: &str) {
        let rows = self.visible(query);
        if rows.is_empty() {
            self.selected = None;
            return;
        }
        let current = self
            .selected
            .as_ref()
            .and_then(|selected| rows.iter().position(|row| &row.path == selected))
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(rows.len().saturating_sub(1));
        self.selected = Some(rows[next].path.clone());
    }

    pub fn select_visible(&mut self, index: usize, query: &str) -> bool {
        let rows = self.visible(query);
        let Some(row) = rows.get(index) else {
            return false;
        };
        self.selected = Some(row.path.clone());
        true
    }

    pub fn activate_selected(&mut self) -> TreeAction {
        let Some(path) = self.selected.clone() else {
            return TreeAction::None;
        };
        let Some(node) = self.nodes.get(&path) else {
            return TreeAction::None;
        };
        match node.kind {
            EntryKind::Error => {
                return TreeAction::Message(
                    node.error
                        .clone()
                        .unwrap_or_else(|| "entry is unavailable".to_owned()),
                );
            }
            EntryKind::File => return TreeAction::OpenFile(path),
            EntryKind::LinkFile => {
                let target = match fs::canonicalize(&path) {
                    Ok(target) => target,
                    Err(error) => return TreeAction::Message(error.to_string()),
                };
                if !path_is_within(&self.root, &target) {
                    return TreeAction::Message(format!(
                        "link target is outside the Workspace Root: {}",
                        target.display()
                    ));
                }
                return TreeAction::OpenFile(path);
            }
            EntryKind::Other => {
                return TreeAction::Message("this filesystem entry cannot be previewed".to_owned());
            }
            EntryKind::Directory | EntryKind::LinkDirectory => {}
        }

        let node = self
            .nodes
            .get_mut(&path)
            .expect("selected node must still exist");
        if node.expanded {
            node.expanded = false;
            return TreeAction::None;
        }
        node.expanded = true;
        if matches!(node.load_state, LoadState::Unloaded | LoadState::Failed) {
            node.load_state = LoadState::Loading;
            TreeAction::LoadDirectory(path)
        } else {
            TreeAction::None
        }
    }

    pub fn expand_selected(&mut self) -> TreeAction {
        let Some(path) = self.selected.clone() else {
            return TreeAction::None;
        };
        let Some(node) = self.nodes.get_mut(&path) else {
            return TreeAction::None;
        };
        if !node.kind.is_directory() || node.expanded {
            return TreeAction::None;
        }
        node.expanded = true;
        if matches!(node.load_state, LoadState::Unloaded | LoadState::Failed) {
            node.load_state = LoadState::Loading;
            TreeAction::LoadDirectory(path)
        } else {
            TreeAction::None
        }
    }

    pub fn collapse_or_parent(&mut self) {
        let Some(path) = self.selected.clone() else {
            return;
        };
        if let Some(node) = self.nodes.get_mut(&path)
            && node.kind.is_directory()
            && node.expanded
        {
            node.expanded = false;
            return;
        }
        if let Some(parent) = path.parent()
            && parent != self.root
            && self.nodes.contains_key(parent)
        {
            self.selected = Some(parent.to_path_buf());
        }
    }

    pub fn toggle_git_directory(&mut self) -> PathBuf {
        self.show_git_directory = !self.show_git_directory;
        if let Some(root) = self.nodes.get_mut(&self.root) {
            root.load_state = LoadState::Loading;
        }
        self.root.clone()
    }

    fn collect_visible(&self, parent: &Path, depth: usize, rows: &mut Vec<VisibleEntry>) {
        let Some(node) = self.nodes.get(parent) else {
            return;
        };
        for child in &node.children {
            let Some(child_node) = self.nodes.get(child) else {
                continue;
            };
            rows.push(self.visible_node(child_node, depth));
            if child_node.kind.is_directory() && child_node.expanded {
                self.collect_visible(child, depth + 1, rows);
            }
        }
    }

    fn visible_node(&self, node: &Node, depth: usize) -> VisibleEntry {
        VisibleEntry {
            path: node.path.clone(),
            name: node.name.to_string_lossy().into_owned(),
            kind: node.kind,
            depth,
            hidden: node.hidden,
            ignored: node.ignored,
            expanded: node.expanded,
            load_state: node.load_state,
            error: node.error.clone(),
        }
    }

    fn depth(&self, path: &Path) -> usize {
        path.strip_prefix(&self.root)
            .map(|relative| relative.components().count().saturating_sub(1))
            .unwrap_or(0)
    }

    fn remove_subtree(&mut self, path: &Path) {
        if let Some(node) = self.nodes.remove(path) {
            for child in node.children {
                self.remove_subtree(&child);
            }
        }
    }
}

pub fn load_directory(root: &Path, path: &Path, show_git_directory: bool) -> DirectoryListing {
    if let Err(message) = validate_link_target(root, path) {
        return DirectoryListing {
            path: path.to_path_buf(),
            entries: Vec::new(),
            error: Some(message),
        };
    }

    let read = match fs::read_dir(path) {
        Ok(read) => read,
        Err(error) => {
            return DirectoryListing {
                path: path.to_path_buf(),
                entries: Vec::new(),
                error: Some(error.to_string()),
            };
        }
    };

    let mut entries = Vec::new();
    for result in read {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => {
                entries.push(EntryInfo {
                    path: path.join(format!("<unreadable-{}>", entries.len())),
                    name: OsString::from("<unreadable entry>"),
                    kind: EntryKind::Error,
                    hidden: false,
                    ignored: false,
                    error: Some(error.to_string()),
                });
                continue;
            }
        };
        let name = entry.file_name();
        if !show_git_directory && name == OsStr::new(".git") {
            continue;
        }
        let entry_path = entry.path();
        match classify(&entry_path) {
            Ok((kind, hidden)) => entries.push(EntryInfo {
                path: entry_path,
                name,
                kind,
                hidden,
                ignored: false,
                error: None,
            }),
            Err(error) => entries.push(EntryInfo {
                path: entry_path,
                name,
                kind: EntryKind::Error,
                hidden: false,
                ignored: false,
                error: Some(error.to_string()),
            }),
        }
    }

    entries.sort_by(
        |left, right| match (left.kind.is_directory(), right.kind.is_directory()) {
            (true, false) => Ordering::Less,
            (false, true) => Ordering::Greater,
            _ => natural_cmp(&left.name.to_string_lossy(), &right.name.to_string_lossy()),
        },
    );

    DirectoryListing {
        path: path.to_path_buf(),
        entries,
        error: None,
    }
}

fn classify(path: &Path) -> std::io::Result<(EntryKind, bool)> {
    let metadata = fs::symlink_metadata(path)?;
    let hidden = is_hidden(path, &metadata);
    let file_type = metadata.file_type();
    if file_type.is_symlink() || is_reparse_point(&metadata) {
        return match fs::metadata(path) {
            Ok(target) if target.is_dir() => Ok((EntryKind::LinkDirectory, hidden)),
            Ok(target) if target.is_file() => Ok((EntryKind::LinkFile, hidden)),
            Ok(_) => Ok((EntryKind::Other, hidden)),
            Err(_) => Ok((EntryKind::Other, hidden)),
        };
    }
    if file_type.is_dir() {
        Ok((EntryKind::Directory, hidden))
    } else if file_type.is_file() {
        Ok((EntryKind::File, hidden))
    } else {
        Ok((EntryKind::Other, hidden))
    }
}

fn validate_link_target(root: &Path, path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(|error| error.to_string())?;
    if !metadata.file_type().is_symlink() && !is_reparse_point(&metadata) {
        return Ok(());
    }
    let target = fs::canonicalize(path).map_err(|error| error.to_string())?;
    if !path_is_within(root, &target) {
        return Err(format!(
            "link target is outside the Workspace Root: {}",
            target.display()
        ));
    }
    let mut ancestor = path.parent();
    while let Some(parent) = ancestor {
        if let Ok(canonical) = fs::canonicalize(parent)
            && canonical == target
        {
            return Err("link cycle detected".to_owned());
        }
        if parent == root {
            break;
        }
        ancestor = parent.parent();
    }
    Ok(())
}

pub fn path_is_within(root: &Path, candidate: &Path) -> bool {
    #[cfg(windows)]
    {
        let root = normalized_windows_path(root);
        let candidate = normalized_windows_path(candidate);
        candidate == root
            || candidate
                .strip_prefix(&root)
                .is_some_and(|remainder| remainder.starts_with('\\'))
    }
    #[cfg(not(windows))]
    {
        candidate == root || candidate.starts_with(root)
    }
}

#[cfg(windows)]
fn normalized_windows_path(path: &Path) -> String {
    let value = path.to_string_lossy().replace('/', "\\");
    let value = if let Some(rest) = value.strip_prefix("\\\\?\\UNC\\") {
        format!("\\\\{rest}")
    } else if let Some(rest) = value.strip_prefix("\\\\?\\") {
        rest.to_owned()
    } else {
        value
    };
    value.to_lowercase()
}

#[cfg(windows)]
fn is_hidden(path: &Path, metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    metadata.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
        || path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

#[cfg(not(windows))]
fn is_hidden(path: &Path, _metadata: &fs::Metadata) -> bool {
    path.file_name()
        .is_some_and(|name| name.to_string_lossy().starts_with('.'))
}

#[cfg(windows)]
fn is_reparse_point(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
    metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
}

#[cfg(not(windows))]
fn is_reparse_point(_metadata: &fs::Metadata) -> bool {
    false
}

fn natural_cmp(left: &str, right: &str) -> Ordering {
    let left_lower = left.to_lowercase();
    let right_lower = right.to_lowercase();
    let mut left_chars = left_lower.chars().peekable();
    let mut right_chars = right_lower.chars().peekable();
    loop {
        match (left_chars.peek(), right_chars.peek()) {
            (Some(a), Some(b)) if a.is_ascii_digit() && b.is_ascii_digit() => {
                let left_number: String = left_chars
                    .by_ref()
                    .take_while(|value| value.is_ascii_digit())
                    .collect();
                let right_number: String = right_chars
                    .by_ref()
                    .take_while(|value| value.is_ascii_digit())
                    .collect();
                let ordering = left_number
                    .trim_start_matches('0')
                    .len()
                    .cmp(&right_number.trim_start_matches('0').len())
                    .then_with(|| left_number.cmp(&right_number));
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (Some(a), Some(b)) => {
                let ordering = a.cmp(b);
                left_chars.next();
                right_chars.next();
                if ordering != Ordering::Equal {
                    return ordering;
                }
            }
            (None, None) => return left.cmp(right),
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order_places_two_before_ten() {
        assert_eq!(natural_cmp("file2", "file10"), Ordering::Less);
    }
}
