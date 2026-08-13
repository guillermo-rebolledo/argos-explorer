use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};

use ignore::WalkBuilder;
use nucleo::{
    Config as NucleoConfig, Injector, Nucleo, Utf32String,
    pattern::{CaseMatching, Normalization},
};

use crate::workspace::path_is_within;

const INDEX_BATCH_SIZE: usize = 1024;

pub fn fuzzy_score(candidate: &str, query: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let candidate = candidate.to_lowercase();
    let query = query.to_lowercase();
    let mut positions = Vec::with_capacity(query.chars().count());
    let mut search_from = 0;
    for needle in query.chars() {
        let relative = candidate[search_from..].find(needle)?;
        let position = search_from + relative;
        positions.push(position);
        search_from = position + needle.len_utf8();
    }
    let gaps = positions
        .windows(2)
        .map(|pair| pair[1].saturating_sub(pair[0] + 1))
        .sum::<usize>();
    Some(positions[0].saturating_mul(2).saturating_add(gaps))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRecord {
    pub path: Arc<PathBuf>,
}

pub struct QuickOpen {
    matcher: Nucleo<PathRecord>,
    injector: Injector<PathRecord>,
    known: HashSet<Arc<PathBuf>>,
    query: String,
    selected: usize,
    scroll: usize,
    indexed_bytes: usize,
    memory_budget: usize,
    indexing: bool,
    partial: bool,
}

impl QuickOpen {
    pub fn new(memory_budget: usize) -> Self {
        let matcher = Nucleo::new(
            NucleoConfig::DEFAULT.match_paths(),
            Arc::new(|| {}),
            None,
            1,
        );
        let injector = matcher.injector();
        Self {
            matcher,
            injector,
            known: HashSet::new(),
            query: String::new(),
            selected: 0,
            scroll: 0,
            indexed_bytes: 0,
            memory_budget,
            indexing: true,
            partial: false,
        }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn indexed_count(&self) -> usize {
        self.known.len()
    }

    pub fn estimated_memory_bytes(&self) -> usize {
        self.indexed_bytes
    }

    pub fn is_indexing(&self) -> bool {
        self.indexing
    }

    pub fn is_partial(&self) -> bool {
        self.partial
    }

    pub fn finish_indexing(&mut self) {
        self.indexing = false;
    }

    pub fn rebuild(&mut self) {
        let query = self.query.clone();
        *self = Self::new(self.memory_budget);
        self.set_query(query);
    }

    pub fn add_paths(&mut self, root: &Path, paths: Vec<PathBuf>) {
        for path in paths {
            if self.known.contains(&path) {
                continue;
            }
            let relative = path.strip_prefix(root).unwrap_or(&path);
            let display = relative.to_string_lossy().replace('\\', "/");
            let text_bytes = if display.is_ascii() { 2 } else { 5 };
            let estimate = display.len().saturating_mul(text_bytes).saturating_add(128);
            if self.indexed_bytes.saturating_add(estimate) > self.memory_budget {
                self.partial = true;
                self.indexing = false;
                break;
            }
            let path = Arc::new(path);
            let record = PathRecord { path: path.clone() };
            self.injector.push(record, move |_value, columns| {
                columns[0] = Utf32String::from(display);
            });
            self.known.insert(path);
            self.indexed_bytes = self.indexed_bytes.saturating_add(estimate);
        }
    }

    pub fn set_query(&mut self, query: String) {
        let append = query.starts_with(&self.query);
        self.query = query;
        self.matcher.pattern.reparse(
            0,
            &self.query,
            CaseMatching::Smart,
            Normalization::Smart,
            append,
        );
        self.selected = 0;
        self.scroll = 0;
    }

    pub fn push_char(&mut self, value: char) {
        let mut query = self.query.clone();
        query.push(value);
        self.set_query(query);
    }

    pub fn pop_char(&mut self) {
        let mut query = self.query.clone();
        query.pop();
        self.set_query(query);
    }

    pub fn tick(&mut self) -> bool {
        self.matcher.tick(1).changed
    }

    pub fn result_count(&self) -> usize {
        self.matcher.snapshot().matched_item_count() as usize
    }

    pub fn results(&self, start: usize, count: usize) -> Vec<PathRecord> {
        let total = self.result_count();
        let start = start.min(total);
        let end = start.saturating_add(count).min(total);
        self.matcher
            .snapshot()
            .matched_items(start as u32..end as u32)
            .map(|item| item.data.clone())
            .collect()
    }

    pub fn selected_record(&self) -> Option<PathRecord> {
        self.matcher
            .snapshot()
            .get_matched_item(self.selected as u32)
            .map(|item| item.data.clone())
    }

    pub fn move_selection(&mut self, delta: isize, viewport_height: usize) {
        let count = self.result_count();
        if count == 0 {
            self.selected = 0;
            self.scroll = 0;
            return;
        }
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(count.saturating_sub(1));
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll.saturating_add(viewport_height.max(1)) {
            self.scroll = self
                .selected
                .saturating_add(1)
                .saturating_sub(viewport_height.max(1));
        }
    }

    pub fn select_visible(&mut self, visible_index: usize) -> bool {
        let selected = self.scroll.saturating_add(visible_index);
        if selected >= self.result_count() {
            return false;
        }
        self.selected = selected;
        true
    }
}

pub fn scan_workspace(root: &Path, mut emit: impl FnMut(Vec<PathBuf>)) {
    let canonical_root = root.to_path_buf();
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .git_global(false)
        .git_exclude(false)
        .parents(false)
        .follow_links(false)
        .filter_entry(|entry| entry.file_name() != ".git");

    let mut batch = Vec::with_capacity(INDEX_BATCH_SIZE);
    for result in builder.build() {
        let Ok(entry) = result else {
            continue;
        };
        let path = entry.into_path();
        if path == canonical_root {
            continue;
        }
        let is_file = match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => std::fs::canonicalize(&path)
                .ok()
                .filter(|target| path_is_within(&canonical_root, target))
                .is_some_and(|target| target.is_file()),
            Ok(metadata) => metadata.is_file(),
            Err(_) => false,
        };
        if !is_file {
            continue;
        }
        batch.push(path);
        if batch.len() >= INDEX_BATCH_SIZE {
            emit(std::mem::take(&mut batch));
            batch = Vec::with_capacity(INDEX_BATCH_SIZE);
        }
    }
    if !batch.is_empty() {
        emit(batch);
    }
}
