use std::{
    collections::{HashMap, HashSet, VecDeque},
    io,
    num::NonZeroUsize,
    path::PathBuf,
    time::{Duration, Instant},
};

use crossterm::event;
use lru::LruCache;

use crate::{
    config::Config,
    external::VscodeCli,
    git::{ChangeEntry, ChangeGroup, DiffOutput, GitRepo},
    search::{PathRecord, QuickOpen},
    terminal::ArgosExplorerTerminal,
    viewer::{
        BinaryDocument, LargeDocument, LoadedDocument, Page, SearchMatch, TextDocument, find_lines,
        text_document_from_string,
    },
    watch::{WatchNotice, WatchService},
    worker::{WorkerCommand, WorkerPool, WorkerResult},
    workspace::{TreeAction, VisibleEntry, WorkspaceTree},
};

const EVENT_POLL: Duration = Duration::from_millis(33);
const WATCH_DEBOUNCE: Duration = Duration::from_millis(250);
const MAX_DIAGNOSTICS: usize = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Files,
    Changes,
    Preview,
    Diff,
    QuickOpen,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Files,
    Changes,
    Preview,
    Diff,
}

#[derive(Debug, Clone)]
pub enum GitState {
    Discovering,
    Ready,
    NotRepository,
    Unavailable(String),
}

#[derive(Debug)]
pub struct LargeFileState {
    pub document: LargeDocument,
    pages: LruCache<u64, Page>,
    pub active_offset: u64,
    pub loading: bool,
    pub search_results: Vec<SearchMatch>,
    pub search_running: bool,
    pub current_search: usize,
}

impl LargeFileState {
    fn new(document: LargeDocument, cache_bytes: usize) -> Self {
        let entries = (cache_bytes / crate::viewer::PAGE_SIZE).max(1);
        Self {
            document,
            pages: LruCache::new(NonZeroUsize::new(entries).expect("positive cache size")),
            active_offset: 0,
            loading: true,
            search_results: Vec::new(),
            search_running: false,
            current_search: 0,
        }
    }

    pub fn active_page(&self) -> Option<&Page> {
        self.pages.peek(&self.active_offset)
    }

    fn insert_page(&mut self, page: Page) {
        self.active_offset = page.offset;
        self.pages.put(page.offset, page);
        self.loading = false;
    }
}

#[derive(Debug)]
pub enum FileContent {
    Empty,
    Loading,
    Text(TextDocument),
    Binary(BinaryDocument),
    Large(LargeFileState),
    Error(String),
}

#[derive(Debug)]
pub struct FileViewerState {
    pub path: Option<PathBuf>,
    pub content: FileContent,
    pub vertical: usize,
    pub horizontal: usize,
    pub wrap: bool,
    pub stale: bool,
    pub query: String,
    pub matches: Vec<usize>,
    pub current_match: usize,
}

impl Default for FileViewerState {
    fn default() -> Self {
        Self {
            path: None,
            content: FileContent::Empty,
            vertical: 0,
            horizontal: 0,
            wrap: false,
            stale: false,
            query: String::new(),
            matches: Vec::new(),
            current_match: 0,
        }
    }
}

#[derive(Debug, Default)]
pub struct DiffState {
    pub entry: Option<ChangeEntry>,
    pub title: String,
    pub document: Option<TextDocument>,
    pub large: Option<LargeFileState>,
    pub binary: bool,
    pub loading: bool,
    pub error: Option<String>,
    pub vertical: usize,
    pub horizontal: usize,
    pub wrap: bool,
    pub query: String,
    pub matches: Vec<usize>,
    pub current_match: usize,
}

#[derive(Debug, Clone)]
pub enum ChangeRow {
    Header { group: ChangeGroup, count: usize },
    Entry { index: usize, entry: ChangeEntry },
}

pub struct App {
    pub config: Config,
    pub screen: Screen,
    pub tree: WorkspaceTree,
    pub tree_filter: String,
    pub tree_scroll: usize,
    pub changes: Vec<ChangeEntry>,
    pub changes_filter: String,
    pub selected_change: Option<usize>,
    pub change_scroll: usize,
    pub git_state: GitState,
    pub viewer: FileViewerState,
    pub diff: DiffState,
    pub quick_open: QuickOpen,
    pub search_mode: Option<SearchMode>,
    pub running: bool,
    pub viewport_width: usize,
    pub viewport_height: usize,
    pub status: String,
    pub diagnostics: VecDeque<String>,
    pub watcher_degraded: bool,
    pub vscode_available: bool,

    workers: WorkerPool,
    watcher: Option<WatchService>,
    repo: Option<GitRepo>,
    vscode: Option<VscodeCli>,
    generation: u64,
    directory_generations: HashMap<PathBuf, u64>,
    git_generation: u64,
    preview_generation: u64,
    page_generation: u64,
    diff_generation: u64,
    diff_page_generation: u64,
    diff_search_generation: u64,
    index_generation: u64,
    search_generation: u64,
    preview_return: Screen,
    diff_return: Screen,
    quick_return: Screen,
    help_return: Screen,
    dirty_since: Option<Instant>,
    dirty_paths: HashSet<PathBuf>,
}

impl App {
    pub fn new(config: Config) -> Self {
        let root = config.root.clone();
        let vscode = VscodeCli::detect();
        let vscode_available = vscode.is_some();
        let watcher_result = WatchService::start(&root);
        let (watcher, watcher_degraded, watcher_message) = match watcher_result {
            Ok(watcher) => (Some(watcher), false, None),
            Err(error) => (
                None,
                true,
                Some(format!("automatic refresh unavailable: {error}")),
            ),
        };
        let mut app = Self {
            tree: WorkspaceTree::new(root.clone()),
            quick_open: QuickOpen::new(config.index_memory_bytes),
            config,
            screen: Screen::Files,
            tree_filter: String::new(),
            tree_scroll: 0,
            changes: Vec::new(),
            changes_filter: String::new(),
            selected_change: None,
            change_scroll: 0,
            git_state: GitState::Discovering,
            viewer: FileViewerState::default(),
            diff: DiffState::default(),
            search_mode: None,
            running: true,
            viewport_width: 80,
            viewport_height: 24,
            status: "Starting…".to_owned(),
            diagnostics: VecDeque::new(),
            watcher_degraded,
            vscode_available,
            workers: WorkerPool::new(),
            watcher,
            repo: None,
            vscode,
            generation: 0,
            directory_generations: HashMap::new(),
            git_generation: 0,
            preview_generation: 0,
            page_generation: 0,
            diff_generation: 0,
            diff_page_generation: 0,
            diff_search_generation: 0,
            index_generation: 0,
            search_generation: 0,
            preview_return: Screen::Files,
            diff_return: Screen::Changes,
            quick_return: Screen::Files,
            help_return: Screen::Files,
            dirty_since: None,
            dirty_paths: HashSet::new(),
        };
        if let Some(message) = watcher_message {
            app.push_diagnostic(message);
        }
        app.request_directory(root.clone());
        let generation = app.next_generation();
        app.workers.submit(WorkerCommand::DiscoverGit {
            generation,
            root: root.clone(),
        });
        app.git_generation = generation;
        app.start_index(root, None);
        app
    }

    pub fn run(&mut self, terminal: &mut ArgosExplorerTerminal) -> io::Result<()> {
        while self.running {
            self.drain_background();
            self.process_watcher();
            self.apply_debounced_refresh();
            self.quick_open.tick();

            terminal.draw(|frame| {
                self.viewport_width = frame.area().width as usize;
                self.viewport_height = frame.area().height.saturating_sub(3) as usize;
                crate::ui::render(frame, self);
            })?;

            if event::poll(EVENT_POLL)? {
                let event = event::read()?;
                crate::input::handle_event(self, event);
            }
        }
        Ok(())
    }

    pub fn visible_tree(&self) -> Vec<VisibleEntry> {
        self.tree.visible(&self.tree_filter)
    }

    pub fn change_rows(&self) -> Vec<ChangeRow> {
        let query = &self.changes_filter;
        let mut rows = Vec::new();
        for group in [
            ChangeGroup::Conflict,
            ChangeGroup::Staged,
            ChangeGroup::Unstaged,
            ChangeGroup::Untracked,
        ] {
            let mut matching: Vec<_> = self
                .changes
                .iter()
                .enumerate()
                .filter(|(_, entry)| entry.group == group)
                .filter_map(|(index, entry)| {
                    crate::search::fuzzy_score(&entry.display_path(), query)
                        .map(|score| (score, index, entry))
                })
                .collect();
            matching.sort_by_key(|item| item.0);
            if matching.is_empty() {
                continue;
            }
            rows.push(ChangeRow::Header {
                group,
                count: matching.len(),
            });
            rows.extend(
                matching
                    .into_iter()
                    .map(|(_, index, entry)| ChangeRow::Entry {
                        index,
                        entry: entry.clone(),
                    }),
            );
        }
        rows
    }

    pub fn quick_results(&self) -> Vec<PathRecord> {
        self.quick_open
            .results(self.quick_open.scroll(), self.viewport_height)
    }

    pub fn set_screen(&mut self, screen: Screen) {
        self.search_mode = None;
        self.screen = screen;
    }

    pub fn open_quick_open(&mut self) {
        if self.screen != Screen::QuickOpen {
            self.quick_return = self.screen;
        }
        self.search_mode = None;
        self.screen = Screen::QuickOpen;
    }

    pub fn open_help(&mut self) {
        if self.screen != Screen::Help {
            self.help_return = self.screen;
            self.screen = Screen::Help;
        }
    }

    pub fn back(&mut self) {
        if self.clear_active_search() {
            return;
        }
        self.screen = match self.screen {
            Screen::Preview => self.preview_return,
            Screen::Diff => self.diff_return,
            Screen::QuickOpen => self.quick_return,
            Screen::Help => self.help_return,
            Screen::Files | Screen::Changes => self.screen,
        };
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn begin_search(&mut self) {
        self.search_mode = match self.screen {
            Screen::Files => Some(SearchMode::Files),
            Screen::Changes => Some(SearchMode::Changes),
            Screen::Preview => Some(SearchMode::Preview),
            Screen::Diff => Some(SearchMode::Diff),
            Screen::QuickOpen | Screen::Help => None,
        };
    }

    pub fn search_char(&mut self, value: char) {
        match self.search_mode {
            Some(SearchMode::Files) => self.tree_filter.push(value),
            Some(SearchMode::Changes) => self.changes_filter.push(value),
            Some(SearchMode::Preview) => {
                self.viewer.query.push(value);
                self.update_viewer_search();
            }
            Some(SearchMode::Diff) => {
                self.diff.query.push(value);
                self.update_diff_search();
            }
            None if self.screen == Screen::QuickOpen => self.quick_open.push_char(value),
            None => {}
        }
        self.normalize_list_selection();
    }

    pub fn search_backspace(&mut self) {
        match self.search_mode {
            Some(SearchMode::Files) => {
                self.tree_filter.pop();
            }
            Some(SearchMode::Changes) => {
                self.changes_filter.pop();
            }
            Some(SearchMode::Preview) => {
                self.viewer.query.pop();
                self.update_viewer_search();
            }
            Some(SearchMode::Diff) => {
                self.diff.query.pop();
                self.update_diff_search();
            }
            None if self.screen == Screen::QuickOpen => self.quick_open.pop_char(),
            None => {}
        }
        self.normalize_list_selection();
    }

    pub fn finish_search(&mut self) {
        if self.search_mode == Some(SearchMode::Preview)
            && matches!(self.viewer.content, FileContent::Large(_))
            && !self.viewer.query.is_empty()
        {
            self.request_large_search();
        }
        if self.search_mode == Some(SearchMode::Diff)
            && self.diff.large.is_some()
            && !self.diff.query.is_empty()
        {
            self.request_large_diff_search();
        }
        self.search_mode = None;
    }

    pub fn move_selection(&mut self, delta: isize) {
        match self.screen {
            Screen::Files => {
                self.tree.move_selection(delta, &self.tree_filter);
                self.ensure_tree_selection_visible();
            }
            Screen::Changes => self.move_change_selection(delta),
            Screen::QuickOpen => self.quick_open.move_selection(delta, self.viewport_height),
            Screen::Preview => self.scroll_file(delta),
            Screen::Diff => self.scroll_diff(delta),
            Screen::Help => {}
        }
    }

    pub fn page(&mut self, direction: isize) {
        let amount = self.viewport_height.saturating_sub(1).max(1) as isize * direction;
        self.move_selection(amount);
    }

    pub fn home(&mut self) {
        match self.screen {
            Screen::Files => self.tree.move_selection(isize::MIN, &self.tree_filter),
            Screen::Changes => self.move_change_selection(isize::MIN),
            Screen::QuickOpen => self
                .quick_open
                .move_selection(isize::MIN, self.viewport_height),
            Screen::Preview => self.viewer.vertical = 0,
            Screen::Diff => self.diff.vertical = 0,
            Screen::Help => {}
        }
    }

    pub fn end(&mut self) {
        match self.screen {
            Screen::Files => self.tree.move_selection(isize::MAX, &self.tree_filter),
            Screen::Changes => self.move_change_selection(isize::MAX),
            Screen::QuickOpen => self
                .quick_open
                .move_selection(isize::MAX, self.viewport_height),
            Screen::Preview => self.file_end(),
            Screen::Diff => self.diff_end(),
            Screen::Help => {}
        }
    }

    pub fn activate(&mut self) {
        match self.screen {
            Screen::Files => {
                let action = self.tree.activate_selected();
                self.handle_tree_action(action);
            }
            Screen::Changes => self.activate_change(),
            Screen::QuickOpen => {
                if let Some(record) = self.quick_open.selected_record() {
                    self.open_file(record.path.as_ref().clone(), Screen::QuickOpen);
                }
            }
            Screen::Preview | Screen::Diff | Screen::Help => {}
        }
    }

    pub fn expand(&mut self) {
        if self.screen == Screen::Files {
            let action = self.tree.expand_selected();
            self.handle_tree_action(action);
        }
    }

    pub fn collapse_or_back(&mut self) {
        if self.screen == Screen::Files {
            self.tree.collapse_or_parent();
        } else {
            self.back();
        }
    }

    pub fn toggle_wrap(&mut self) {
        match self.screen {
            Screen::Preview => self.viewer.wrap = !self.viewer.wrap,
            Screen::Diff => self.diff.wrap = !self.diff.wrap,
            _ => {}
        }
    }

    pub fn horizontal_scroll(&mut self, delta: isize) {
        match self.screen {
            Screen::Preview => {
                self.viewer.horizontal = self.viewer.horizontal.saturating_add_signed(delta)
            }
            Screen::Diff => {
                self.diff.horizontal = self.diff.horizontal.saturating_add_signed(delta)
            }
            _ => {}
        }
    }

    pub fn next_match_or_hunk(&mut self, direction: isize) {
        match self.screen {
            Screen::Preview if !self.viewer.matches.is_empty() => {
                self.viewer.current_match = wrap_index(
                    self.viewer.current_match,
                    direction,
                    self.viewer.matches.len(),
                );
                self.viewer.vertical = self.viewer.matches[self.viewer.current_match];
            }
            Screen::Preview => self.navigate_large_match(direction),
            Screen::Diff if !self.diff.matches.is_empty() => {
                self.diff.current_match =
                    wrap_index(self.diff.current_match, direction, self.diff.matches.len());
                self.diff.vertical = self.diff.matches[self.diff.current_match];
            }
            Screen::Diff
                if self
                    .diff
                    .large
                    .as_ref()
                    .is_some_and(|large| !large.search_results.is_empty()) =>
            {
                self.navigate_large_diff_match(direction);
            }
            Screen::Diff => self.jump_hunk(direction),
            _ => {}
        }
    }

    pub fn reload_active(&mut self) {
        match self.screen {
            Screen::Files => {
                for path in self.tree.expanded_directories() {
                    self.request_directory(path);
                }
            }
            Screen::Changes | Screen::Diff => self.request_git_status(),
            Screen::Preview => {
                if let Some(path) = self.viewer.path.clone() {
                    let return_to = self.preview_return;
                    self.open_file(path, return_to);
                }
            }
            Screen::QuickOpen => self.rebuild_index(),
            Screen::Help => {}
        }
    }

    pub fn full_refresh(&mut self) {
        for path in self.tree.expanded_directories() {
            self.request_directory(path);
        }
        self.request_git_status();
        self.rebuild_index();
        self.set_status("Full refresh started");
    }

    pub fn toggle_git_directory(&mut self) {
        let root = self.tree.toggle_git_directory();
        self.request_directory(root);
    }

    pub fn open_vscode(&mut self) {
        let Some(vscode) = self.vscode.clone() else {
            self.set_status("VS Code CLI is not available");
            return;
        };
        let workspace = PathBuf::from(crate::config::display_path(&self.config.root));
        match vscode.open_workspace(&workspace) {
            Ok(_) => self.set_status("Opened Workspace Root in VS Code"),
            Err(error) => self.set_status(format!("Could not open VS Code: {error}")),
        }
    }

    pub fn click(&mut self, column: u16, row: u16) {
        if row == 0 {
            let full_screen = matches!(self.screen, Screen::Preview | Screen::Diff | Screen::Help);
            if full_screen {
                if column <= 9 {
                    self.back();
                } else if column as usize >= self.viewport_width.saturating_sub(8) {
                    self.quit();
                }
                return;
            }
            match column {
                0..=6 => self.set_screen(Screen::Files),
                8..=16 => self.set_screen(Screen::Changes),
                18..=29 => self.open_quick_open(),
                31..=38 if self.vscode_available && self.viewport_width >= 48 => self.open_vscode(),
                value if value as usize >= self.viewport_width.saturating_sub(8) => self.quit(),
                _ => {}
            }
            return;
        }
        if row < 2 || row as usize >= self.viewport_height.saturating_add(2) {
            return;
        }
        let visible_row = row as usize - 2;
        match self.screen {
            Screen::Files => {
                if self.tree.select_visible(
                    self.tree_scroll.saturating_add(visible_row),
                    &self.tree_filter,
                ) {
                    self.activate();
                }
            }
            Screen::Changes => {
                let rows = self.change_rows();
                if let Some(ChangeRow::Entry { index, .. }) =
                    rows.get(self.change_scroll.saturating_add(visible_row))
                {
                    self.selected_change = Some(*index);
                    self.activate_change();
                }
            }
            Screen::QuickOpen => {
                if self.quick_open.select_visible(visible_row) {
                    self.activate();
                }
            }
            Screen::Preview | Screen::Diff | Screen::Help => {}
        }
    }

    pub fn set_status(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.status.clone_from(&message);
        self.push_diagnostic(message);
    }

    pub fn git_status_label(&self) -> String {
        match &self.git_state {
            GitState::Discovering => "Git: discovering…".to_owned(),
            GitState::Ready => format!("Git: {} changes", self.changes.len()),
            GitState::NotRepository => "Not a Git repository".to_owned(),
            GitState::Unavailable(message) => format!("Git unavailable: {message}"),
        }
    }

    pub fn search_prompt(&self) -> Option<(&'static str, &str)> {
        match self.search_mode {
            Some(SearchMode::Files) => Some(("Filter files", &self.tree_filter)),
            Some(SearchMode::Changes) => Some(("Filter changes", &self.changes_filter)),
            Some(SearchMode::Preview) => Some(("Find", &self.viewer.query)),
            Some(SearchMode::Diff) => Some(("Find", &self.diff.query)),
            None => None,
        }
    }

    fn handle_tree_action(&mut self, action: TreeAction) {
        match action {
            TreeAction::None => {}
            TreeAction::LoadDirectory(path) => self.request_directory(path),
            TreeAction::OpenFile(path) => self.open_file(path, Screen::Files),
            TreeAction::Message(message) => self.set_status(message),
        }
    }

    fn open_file(&mut self, path: PathBuf, return_to: Screen) {
        self.preview_return = return_to;
        self.screen = Screen::Preview;
        self.viewer.path = Some(path.clone());
        self.viewer.content = FileContent::Loading;
        self.viewer.vertical = 0;
        self.viewer.horizontal = 0;
        self.viewer.stale = false;
        self.viewer.query.clear();
        self.viewer.matches.clear();
        let generation = self.next_generation();
        self.preview_generation = generation;
        if !self.workers.submit(WorkerCommand::LoadFile {
            generation,
            path,
            small_file_limit: self.config.small_file_limit,
        }) {
            self.viewer.content = FileContent::Error("background queue is full".to_owned());
        }
    }

    fn activate_change(&mut self) {
        let Some(index) = self.selected_change else {
            return;
        };
        let Some(entry) = self.changes.get(index).cloned() else {
            return;
        };
        let Some(repo) = self.repo.clone() else {
            self.set_status("Changes are unavailable outside a Git repository");
            return;
        };
        self.diff_return = Screen::Changes;
        self.screen = Screen::Diff;
        self.diff = DiffState {
            entry: Some(entry.clone()),
            title: entry.display_path(),
            loading: true,
            ..DiffState::default()
        };
        let generation = self.next_generation();
        self.diff_generation = generation;
        if !self.workers.submit(WorkerCommand::LoadDiff {
            generation,
            repo,
            entry,
            small_file_limit: self.config.small_file_limit,
        }) {
            self.diff.loading = false;
            self.diff.error = Some("background queue is full".to_owned());
        }
    }

    fn request_directory(&mut self, path: PathBuf) {
        let generation = self.next_generation();
        self.directory_generations.insert(path.clone(), generation);
        self.tree.mark_loading(&path);
        let command = WorkerCommand::LoadDirectory {
            generation,
            root: self.config.root.clone(),
            path,
            show_git_directory: self.tree.show_git_directory,
            repo: self.repo.clone(),
        };
        if !self.workers.submit(command) {
            self.set_status("background queue is full; directory refresh skipped");
        }
    }

    fn request_git_status(&mut self) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        let generation = self.next_generation();
        self.git_generation = generation;
        if !self
            .workers
            .submit(WorkerCommand::RefreshGit { generation, repo })
        {
            self.set_status("background queue is full; Git refresh skipped");
        }
    }

    fn request_page(&mut self, document: LargeDocument, offset: u64) {
        let generation = self.next_generation();
        self.page_generation = generation;
        if let FileContent::Large(large) = &mut self.viewer.content {
            large.loading = true;
        }
        if !self.workers.submit(WorkerCommand::LoadPage {
            generation,
            document,
            offset,
        }) {
            self.set_status("background queue is full; page load skipped");
        }
    }

    fn request_diff_page(&mut self, document: LargeDocument, offset: u64) {
        let generation = self.next_generation();
        self.diff_page_generation = generation;
        if let Some(large) = &mut self.diff.large {
            large.loading = true;
        }
        if !self.workers.submit(WorkerCommand::LoadDiffPage {
            generation,
            document,
            offset,
        }) {
            self.set_status("background queue is full; diff page load skipped");
        }
    }

    fn request_large_search(&mut self) {
        let (path, query) = match &mut self.viewer.content {
            FileContent::Large(large) => {
                large.search_running = true;
                (large.document.path.clone(), self.viewer.query.clone())
            }
            _ => return,
        };
        let generation = self.next_generation();
        self.search_generation = generation;
        if !self.workers.submit(WorkerCommand::SearchLarge {
            generation,
            path,
            query,
        }) {
            self.set_status("background queue is full; search skipped");
        }
    }

    fn request_large_diff_search(&mut self) {
        let Some(large) = &mut self.diff.large else {
            return;
        };
        large.search_running = true;
        let path = large.document.path.clone();
        let query = self.diff.query.clone();
        let generation = self.next_generation();
        self.diff_search_generation = generation;
        if !self.workers.submit(WorkerCommand::SearchLargeDiff {
            generation,
            path,
            query,
        }) {
            self.set_status("background queue is full; diff search skipped");
        }
    }

    fn start_index(&mut self, root: PathBuf, repo: Option<GitRepo>) {
        let generation = self.next_generation();
        self.index_generation = generation;
        self.workers.submit(WorkerCommand::BuildIndex {
            generation,
            root,
            repo,
        });
    }

    fn rebuild_index(&mut self) {
        self.quick_open.rebuild();
        self.start_index(self.config.root.clone(), self.repo.clone());
    }

    fn drain_background(&mut self) {
        let results: Vec<_> = self.workers.try_iter().collect();
        for result in results {
            match result {
                WorkerResult::GitDiscovered { generation, result }
                    if generation == self.git_generation =>
                {
                    match result {
                        Ok(Some(repo)) => {
                            self.repo = Some(repo.clone());
                            self.git_state = GitState::Ready;
                            self.request_git_status();
                            self.workers.submit(WorkerCommand::SeedIndex {
                                generation: self.index_generation,
                                repo,
                            });
                        }
                        Ok(None) => {
                            self.git_state = GitState::NotRepository;
                            self.status = "Workspace ready".to_owned();
                        }
                        Err(error) => {
                            self.git_state = GitState::Unavailable(error.clone());
                            self.set_status(error);
                        }
                    }
                }
                WorkerResult::DirectoryLoaded {
                    generation,
                    listing,
                } if self.directory_generations.get(&listing.path) == Some(&generation) => {
                    self.tree.apply_listing(listing);
                    self.status = "Workspace ready".to_owned();
                }
                WorkerResult::FileLoaded {
                    generation,
                    path,
                    result,
                } if generation == self.preview_generation
                    && self.viewer.path.as_deref() == Some(path.as_path()) =>
                {
                    if result.is_ok() {
                        self.viewer.stale = false;
                    }
                    match result {
                        Ok(LoadedDocument::Text(document)) => {
                            self.viewer.content = FileContent::Text(document)
                        }
                        Ok(LoadedDocument::Binary(document)) => {
                            self.viewer.content = FileContent::Binary(document)
                        }
                        Ok(LoadedDocument::Large(document)) => {
                            let page_document = document.clone();
                            self.viewer.content = FileContent::Large(LargeFileState::new(
                                document,
                                self.config.page_cache_bytes,
                            ));
                            self.request_page(page_document, 0);
                        }
                        Err(error) => self.viewer.content = FileContent::Error(error),
                    }
                }
                WorkerResult::PageLoaded {
                    generation,
                    path,
                    result,
                } if generation == self.page_generation
                    && self.viewer.path.as_deref() == Some(path.as_path()) =>
                {
                    match result {
                        Ok(page) => {
                            if let FileContent::Large(large) = &mut self.viewer.content {
                                large.insert_page(page);
                                self.viewer.vertical = 0;
                            }
                        }
                        Err(error) => self.set_status(error),
                    }
                }
                WorkerResult::DiffPageLoaded {
                    generation,
                    path,
                    result,
                } if generation == self.diff_page_generation
                    && self
                        .diff
                        .large
                        .as_ref()
                        .is_some_and(|large| large.document.path == path) =>
                {
                    match result {
                        Ok(page) => {
                            if let Some(large) = &mut self.diff.large {
                                large.insert_page(page);
                                self.diff.vertical = 0;
                            }
                        }
                        Err(error) => self.set_status(error),
                    }
                }
                WorkerResult::LargeSearchFinished { generation, result }
                    if generation == self.search_generation =>
                {
                    match result {
                        Ok(matches) => {
                            let count = matches.len();
                            let request =
                                if let FileContent::Large(large) = &mut self.viewer.content {
                                    large.search_results = matches;
                                    large.search_running = false;
                                    large.current_search = 0;
                                    large
                                        .search_results
                                        .first()
                                        .map(|found| (large.document.clone(), found.byte_offset))
                                } else {
                                    None
                                };
                            self.set_status(format!("{count} matches found"));
                            if let Some((document, offset)) = request {
                                self.request_page(document, offset);
                            }
                        }
                        Err(error) => self.set_status(error),
                    }
                }
                WorkerResult::LargeDiffSearchFinished { generation, result }
                    if generation == self.diff_search_generation =>
                {
                    match result {
                        Ok(matches) => {
                            let count = matches.len();
                            let request = if let Some(large) = &mut self.diff.large {
                                large.search_results = matches;
                                large.search_running = false;
                                large.current_search = 0;
                                large
                                    .search_results
                                    .first()
                                    .map(|found| (large.document.clone(), found.byte_offset))
                            } else {
                                None
                            };
                            self.set_status(format!("{count} diff matches found"));
                            if let Some((document, offset)) = request {
                                self.request_diff_page(document, offset);
                            }
                        }
                        Err(error) => self.set_status(error),
                    }
                }
                WorkerResult::GitStatus { generation, result }
                    if generation == self.git_generation =>
                {
                    match result {
                        Ok(changes) => {
                            self.changes = changes;
                            self.git_state = GitState::Ready;
                            self.normalize_list_selection();
                        }
                        Err(error) => {
                            self.git_state = GitState::Unavailable(error.clone());
                            self.set_status(error);
                        }
                    }
                }
                WorkerResult::DiffLoaded {
                    generation,
                    entry,
                    result,
                } if generation == self.diff_generation
                    && self.diff.entry.as_ref() == Some(&entry) =>
                {
                    self.apply_diff(result);
                }
                WorkerResult::IndexBatch { generation, paths }
                    if generation == self.index_generation =>
                {
                    self.quick_open.add_paths(&self.config.root, paths);
                }
                WorkerResult::IndexFinished { generation }
                    if generation == self.index_generation =>
                {
                    self.quick_open.finish_indexing();
                }
                _ => {}
            }
        }
    }

    fn apply_diff(&mut self, result: Result<DiffOutput, String>) {
        self.diff.loading = false;
        match result {
            Ok(output) => {
                self.diff.title = output.title;
                self.diff.binary = output.binary;
                self.diff.error = None;
                self.diff.vertical = 0;
                self.diff.horizontal = 0;
                if let Some(document) = output.large_untracked {
                    let page_document = document.clone();
                    self.diff.document = None;
                    self.diff.large =
                        Some(LargeFileState::new(document, self.config.page_cache_bytes));
                    self.request_diff_page(page_document, 0);
                } else {
                    let path = self
                        .diff
                        .entry
                        .as_ref()
                        .map(|entry| entry.path.clone())
                        .unwrap_or_else(|| PathBuf::from("diff"));
                    self.diff.large = None;
                    self.diff.document = Some(text_document_from_string(path, output.text));
                }
            }
            Err(error) => self.diff.error = Some(error),
        }
    }

    fn process_watcher(&mut self) {
        let notices: Vec<_> = self
            .watcher
            .as_ref()
            .map(|watcher| watcher.try_iter().collect())
            .unwrap_or_default();
        for notice in notices {
            match notice {
                WatchNotice::Changed(paths) => {
                    for path in paths {
                        if self.viewer.path.as_ref() == Some(&path) {
                            self.viewer.stale = true;
                        }
                        self.dirty_paths.insert(path);
                    }
                    self.dirty_since.get_or_insert_with(Instant::now);
                }
                WatchNotice::Error(error) => {
                    self.watcher_degraded = true;
                    self.set_status(format!("automatic refresh degraded: {error}"));
                }
            }
        }
    }

    fn apply_debounced_refresh(&mut self) {
        let Some(since) = self.dirty_since else {
            return;
        };
        if since.elapsed() < WATCH_DEBOUNCE {
            return;
        }
        self.dirty_since = None;
        let dirty = std::mem::take(&mut self.dirty_paths);
        let mut directories = HashSet::new();
        for path in &dirty {
            if path.is_file() {
                self.quick_open
                    .add_paths(&self.config.root, vec![path.clone()]);
            }
            let mut parent = path.parent();
            while let Some(candidate) = parent {
                if self.tree.has_node(candidate) {
                    directories.insert(candidate.to_path_buf());
                    break;
                }
                if candidate == self.config.root {
                    break;
                }
                parent = candidate.parent();
            }
        }
        for directory in directories {
            self.request_directory(directory);
        }
        self.request_git_status();
    }

    fn scroll_file(&mut self, delta: isize) {
        match &mut self.viewer.content {
            FileContent::Text(document) => {
                self.viewer.vertical = self
                    .viewer
                    .vertical
                    .saturating_add_signed(delta)
                    .min(document.line_count().saturating_sub(1));
            }
            FileContent::Large(large) => {
                let line_count = large
                    .active_page()
                    .map(|page| page.lines.len())
                    .unwrap_or(0);
                let next = self.viewer.vertical.saturating_add_signed(delta);
                if next < line_count {
                    self.viewer.vertical = next;
                    return;
                }
                let request = large
                    .active_page()
                    .map(|page| page.next_offset)
                    .filter(|offset| *offset < large.document.size);
                if delta > 0 {
                    if let Some(offset) = request {
                        let document = large.document.clone();
                        self.request_page(document, offset);
                    }
                } else if delta < 0 && large.active_offset > 0 {
                    let offset = large
                        .active_offset
                        .saturating_sub(crate::viewer::PAGE_SIZE as u64);
                    let document = large.document.clone();
                    self.request_page(document, offset);
                }
            }
            FileContent::Empty
            | FileContent::Loading
            | FileContent::Binary(_)
            | FileContent::Error(_) => {}
        }
    }

    fn scroll_diff(&mut self, delta: isize) {
        let request = if let Some(large) = &mut self.diff.large {
            let line_count = large
                .active_page()
                .map(|page| page.lines.len())
                .unwrap_or(0);
            let next = self.diff.vertical.saturating_add_signed(delta);
            if next < line_count {
                self.diff.vertical = next;
                return;
            }
            if delta > 0 {
                large
                    .active_page()
                    .map(|page| page.next_offset)
                    .filter(|offset| *offset < large.document.size)
                    .map(|offset| (large.document.clone(), offset))
            } else if delta < 0 && large.active_offset > 0 {
                Some((
                    large.document.clone(),
                    large
                        .active_offset
                        .saturating_sub(crate::viewer::PAGE_SIZE as u64),
                ))
            } else {
                None
            }
        } else {
            let count = self
                .diff
                .document
                .as_ref()
                .map(TextDocument::line_count)
                .unwrap_or(0);
            self.diff.vertical = self
                .diff
                .vertical
                .saturating_add_signed(delta)
                .min(count.saturating_sub(1));
            None
        };
        if let Some((document, offset)) = request {
            self.request_diff_page(document, offset);
        }
    }

    fn file_end(&mut self) {
        match &mut self.viewer.content {
            FileContent::Text(document) => {
                self.viewer.vertical = document.line_count().saturating_sub(1)
            }
            FileContent::Large(large) => {
                let offset = large
                    .document
                    .size
                    .saturating_sub(crate::viewer::PAGE_SIZE as u64);
                let document = large.document.clone();
                self.request_page(document, offset);
            }
            _ => {}
        }
    }

    fn diff_end(&mut self) {
        if let Some(large) = &self.diff.large {
            let offset = large
                .document
                .size
                .saturating_sub(crate::viewer::PAGE_SIZE as u64);
            self.request_diff_page(large.document.clone(), offset);
        } else {
            self.diff.vertical = self
                .diff
                .document
                .as_ref()
                .map(TextDocument::line_count)
                .unwrap_or(1)
                .saturating_sub(1);
        }
    }

    fn move_change_selection(&mut self, delta: isize) {
        let indices: Vec<_> = self
            .change_rows()
            .into_iter()
            .filter_map(|row| match row {
                ChangeRow::Entry { index, .. } => Some(index),
                ChangeRow::Header { .. } => None,
            })
            .collect();
        if indices.is_empty() {
            self.selected_change = None;
            return;
        }
        let current = self
            .selected_change
            .and_then(|selected| indices.iter().position(|index| *index == selected))
            .unwrap_or(0);
        let next = current
            .saturating_add_signed(delta)
            .min(indices.len().saturating_sub(1));
        self.selected_change = Some(indices[next]);
        let rows = self.change_rows();
        if let Some(row_index) = rows.iter().position(
            |row| matches!(row, ChangeRow::Entry { index, .. } if Some(*index) == self.selected_change),
        ) {
            ensure_visible(&mut self.change_scroll, row_index, self.viewport_height);
        }
    }

    fn normalize_list_selection(&mut self) {
        let visible_change = self.change_rows().into_iter().find_map(|row| match row {
            ChangeRow::Entry { index, .. } => Some(index),
            ChangeRow::Header { .. } => None,
        });
        if self.selected_change.is_none_or(|selected| {
            !self
                .change_rows()
                .iter()
                .any(|row| matches!(row, ChangeRow::Entry { index, .. } if *index == selected))
        }) {
            self.selected_change = visible_change;
        }
        self.ensure_tree_selection_visible();
    }

    fn ensure_tree_selection_visible(&mut self) {
        let rows = self.visible_tree();
        if let Some(selected) = self.tree.selected_path()
            && let Some(index) = rows.iter().position(|row| row.path == selected)
        {
            ensure_visible(&mut self.tree_scroll, index, self.viewport_height);
        }
    }

    fn update_viewer_search(&mut self) {
        self.viewer.matches = match &self.viewer.content {
            FileContent::Text(document) => find_lines(&document.text, &self.viewer.query),
            _ => Vec::new(),
        };
        self.viewer.current_match = 0;
        if let Some(first) = self.viewer.matches.first() {
            self.viewer.vertical = *first;
        }
    }

    fn update_diff_search(&mut self) {
        self.diff.matches = self
            .diff
            .document
            .as_ref()
            .map(|document| find_lines(&document.text, &self.diff.query))
            .unwrap_or_default();
        self.diff.current_match = 0;
        if let Some(first) = self.diff.matches.first() {
            self.diff.vertical = *first;
        }
    }

    fn clear_active_search(&mut self) -> bool {
        if self.search_mode.take().is_some() {
            match self.screen {
                Screen::Files => self.tree_filter.clear(),
                Screen::Changes => self.changes_filter.clear(),
                Screen::Preview => {
                    self.viewer.query.clear();
                    self.viewer.matches.clear();
                    if let FileContent::Large(large) = &mut self.viewer.content {
                        large.search_results.clear();
                        large.current_search = 0;
                    }
                }
                Screen::Diff => {
                    self.diff.query.clear();
                    self.diff.matches.clear();
                    if let Some(large) = &mut self.diff.large {
                        large.search_results.clear();
                        large.current_search = 0;
                    }
                }
                _ => {}
            }
            return true;
        }
        false
    }

    fn navigate_large_match(&mut self, direction: isize) {
        let request = match &mut self.viewer.content {
            FileContent::Large(large) if !large.search_results.is_empty() => {
                large.current_search =
                    wrap_index(large.current_search, direction, large.search_results.len());
                let found = &large.search_results[large.current_search];
                Some((large.document.clone(), found.byte_offset))
            }
            _ => None,
        };
        if let Some((document, offset)) = request {
            self.request_page(document, offset);
        }
    }

    fn navigate_large_diff_match(&mut self, direction: isize) {
        let request = match &mut self.diff.large {
            Some(large) if !large.search_results.is_empty() => {
                large.current_search =
                    wrap_index(large.current_search, direction, large.search_results.len());
                let found = &large.search_results[large.current_search];
                Some((large.document.clone(), found.byte_offset))
            }
            _ => None,
        };
        if let Some((document, offset)) = request {
            self.request_diff_page(document, offset);
        }
    }

    fn jump_hunk(&mut self, direction: isize) {
        let Some(document) = &self.diff.document else {
            return;
        };
        let hunks: Vec<_> = (0..document.line_count())
            .filter(|index| {
                document
                    .line(*index)
                    .is_some_and(|line| line.starts_with("@@"))
            })
            .collect();
        if hunks.is_empty() {
            return;
        }
        let target = if direction > 0 {
            hunks
                .iter()
                .copied()
                .find(|line| *line > self.diff.vertical)
                .unwrap_or(hunks[0])
        } else {
            hunks
                .iter()
                .rev()
                .copied()
                .find(|line| *line < self.diff.vertical)
                .unwrap_or(*hunks.last().unwrap_or(&0))
        };
        self.diff.vertical = target;
    }

    fn next_generation(&mut self) -> u64 {
        self.generation = self.generation.wrapping_add(1).max(1);
        self.generation
    }

    fn push_diagnostic(&mut self, message: String) {
        if self.diagnostics.len() >= MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(message);
    }
}

fn ensure_visible(scroll: &mut usize, selected: usize, height: usize) {
    let height = height.max(1);
    if selected < *scroll {
        *scroll = selected;
    } else if selected >= scroll.saturating_add(height) {
        *scroll = selected.saturating_add(1).saturating_sub(height);
    }
}

fn wrap_index(current: usize, delta: isize, length: usize) -> usize {
    if length == 0 {
        return 0;
    }
    if delta >= 0 {
        (current + delta as usize) % length
    } else {
        (current + length - ((-delta) as usize % length)) % length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ensure_visible_scrolls_only_when_needed() {
        let mut scroll = 0;
        ensure_visible(&mut scroll, 9, 5);
        assert_eq!(scroll, 5);
        ensure_visible(&mut scroll, 6, 5);
        assert_eq!(scroll, 5);
    }

    #[test]
    fn wrapped_navigation_cycles() {
        assert_eq!(wrap_index(0, -1, 3), 2);
        assert_eq!(wrap_index(2, 1, 3), 0);
    }
}
