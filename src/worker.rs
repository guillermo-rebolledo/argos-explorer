use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread,
};

use crossbeam_channel::{Receiver, Sender, bounded};

use crate::{
    git::{ChangeEntry, DiffOutput, GitRepo},
    search::scan_workspace,
    viewer::{LargeDocument, LoadedDocument, Page, SearchMatch},
    workspace::DirectoryListing,
};

#[derive(Debug, Clone)]
pub enum WorkerCommand {
    DiscoverGit {
        generation: u64,
        root: PathBuf,
    },
    LoadDirectory {
        generation: u64,
        root: PathBuf,
        path: PathBuf,
        show_git_directory: bool,
        repo: Option<GitRepo>,
    },
    LoadFile {
        generation: u64,
        path: PathBuf,
        small_file_limit: u64,
    },
    LoadPage {
        generation: u64,
        document: LargeDocument,
        offset: u64,
    },
    SearchLarge {
        generation: u64,
        path: PathBuf,
        query: String,
    },
    RefreshGit {
        generation: u64,
        repo: GitRepo,
    },
    LoadDiff {
        generation: u64,
        repo: GitRepo,
        entry: ChangeEntry,
        small_file_limit: u64,
    },
    LoadDiffPage {
        generation: u64,
        document: LargeDocument,
        offset: u64,
    },
    SearchLargeDiff {
        generation: u64,
        path: PathBuf,
        query: String,
    },
    BuildIndex {
        generation: u64,
        root: PathBuf,
        repo: Option<GitRepo>,
    },
    SeedIndex {
        generation: u64,
        repo: GitRepo,
    },
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum WorkerResult {
    GitDiscovered {
        generation: u64,
        result: Result<Option<GitRepo>, String>,
    },
    DirectoryLoaded {
        generation: u64,
        listing: DirectoryListing,
    },
    FileLoaded {
        generation: u64,
        path: PathBuf,
        result: Result<LoadedDocument, String>,
    },
    PageLoaded {
        generation: u64,
        path: PathBuf,
        result: Result<Page, String>,
    },
    LargeSearchFinished {
        generation: u64,
        result: Result<Vec<SearchMatch>, String>,
    },
    GitStatus {
        generation: u64,
        result: Result<Vec<ChangeEntry>, String>,
    },
    DiffLoaded {
        generation: u64,
        entry: ChangeEntry,
        result: Result<DiffOutput, String>,
    },
    DiffPageLoaded {
        generation: u64,
        path: PathBuf,
        result: Result<Page, String>,
    },
    LargeDiffSearchFinished {
        generation: u64,
        result: Result<Vec<SearchMatch>, String>,
    },
    IndexBatch {
        generation: u64,
        paths: Vec<PathBuf>,
    },
    IndexFinished {
        generation: u64,
    },
}

pub struct WorkerPool {
    sender: Sender<WorkerCommand>,
    receiver: Receiver<WorkerResult>,
    threads: Vec<thread::JoinHandle<()>>,
    latest_index_generation: Arc<AtomicU64>,
}

impl WorkerPool {
    pub fn new() -> Self {
        let worker_count = thread::available_parallelism()
            .map(|count| count.get())
            .unwrap_or(4)
            .clamp(2, 8);
        let (command_sender, command_receiver) = bounded(1024);
        let (result_sender, result_receiver) = bounded(4096);
        let latest_index_generation = Arc::new(AtomicU64::new(0));
        let mut threads = Vec::with_capacity(worker_count);
        for index in 0..worker_count {
            let commands = command_receiver.clone();
            let results = result_sender.clone();
            let latest_index = latest_index_generation.clone();
            let handle = thread::Builder::new()
                .name(format!("argos-explorer-worker-{index}"))
                .spawn(move || worker_loop(commands, results, latest_index))
                .expect("worker thread creation should succeed");
            threads.push(handle);
        }
        Self {
            sender: command_sender,
            receiver: result_receiver,
            threads,
            latest_index_generation,
        }
    }

    pub fn submit(&self, command: WorkerCommand) -> bool {
        if let WorkerCommand::BuildIndex { generation, .. } = &command {
            self.latest_index_generation
                .store(*generation, Ordering::Release);
        }
        self.sender.try_send(command).is_ok()
    }

    pub fn try_iter(&self) -> impl Iterator<Item = WorkerResult> + '_ {
        self.receiver.try_iter()
    }
}

impl Default for WorkerPool {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        for _ in 0..self.threads.len() {
            let _ = self.sender.send(WorkerCommand::Shutdown);
        }
        for thread in self.threads.drain(..) {
            let _ = thread.join();
        }
    }
}

fn worker_loop(
    commands: Receiver<WorkerCommand>,
    results: Sender<WorkerResult>,
    latest_index_generation: Arc<AtomicU64>,
) {
    while let Ok(command) = commands.recv() {
        match command {
            WorkerCommand::DiscoverGit { generation, root } => {
                let result = GitRepo::discover(&root).map_err(|error| error.to_string());
                send(&results, WorkerResult::GitDiscovered { generation, result });
            }
            WorkerCommand::LoadDirectory {
                generation,
                root,
                path,
                show_git_directory,
                repo,
            } => {
                let mut listing =
                    crate::workspace::load_directory(&root, &path, show_git_directory);
                if listing.error.is_none()
                    && let Some(repo) = repo
                    && let Err(error) = repo.mark_ignored(&mut listing)
                {
                    tracing::debug!(%error, "could not mark ignored entries");
                }
                send(
                    &results,
                    WorkerResult::DirectoryLoaded {
                        generation,
                        listing,
                    },
                );
            }
            WorkerCommand::LoadFile {
                generation,
                path,
                small_file_limit,
            } => {
                let result = crate::viewer::load_document(&path, small_file_limit)
                    .map_err(|error| error.to_string());
                send(
                    &results,
                    WorkerResult::FileLoaded {
                        generation,
                        path,
                        result,
                    },
                );
            }
            WorkerCommand::LoadPage {
                generation,
                document,
                offset,
            } => {
                let path = document.path.clone();
                let result = crate::viewer::load_page(&document, offset, crate::viewer::PAGE_SIZE)
                    .map_err(|error| error.to_string());
                send(
                    &results,
                    WorkerResult::PageLoaded {
                        generation,
                        path,
                        result,
                    },
                );
            }
            WorkerCommand::SearchLarge {
                generation,
                path,
                query,
            } => {
                let result = crate::viewer::search_large_file(&path, &query)
                    .map_err(|error| error.to_string());
                send(
                    &results,
                    WorkerResult::LargeSearchFinished { generation, result },
                );
            }
            WorkerCommand::RefreshGit { generation, repo } => {
                let result = repo.status().map_err(|error| error.to_string());
                send(&results, WorkerResult::GitStatus { generation, result });
            }
            WorkerCommand::LoadDiff {
                generation,
                repo,
                entry,
                small_file_limit,
            } => {
                let result = repo
                    .diff(&entry, small_file_limit)
                    .map_err(|error| error.to_string());
                send(
                    &results,
                    WorkerResult::DiffLoaded {
                        generation,
                        entry,
                        result,
                    },
                );
            }
            WorkerCommand::LoadDiffPage {
                generation,
                document,
                offset,
            } => {
                let path = document.path.clone();
                let result = crate::viewer::load_page(&document, offset, crate::viewer::PAGE_SIZE)
                    .map_err(|error| error.to_string());
                send(
                    &results,
                    WorkerResult::DiffPageLoaded {
                        generation,
                        path,
                        result,
                    },
                );
            }
            WorkerCommand::SearchLargeDiff {
                generation,
                path,
                query,
            } => {
                let result = crate::viewer::search_large_file(&path, &query)
                    .map_err(|error| error.to_string());
                send(
                    &results,
                    WorkerResult::LargeDiffSearchFinished { generation, result },
                );
            }
            WorkerCommand::BuildIndex {
                generation,
                root,
                repo,
            } => {
                if let Some(repo) = repo
                    && let Ok(paths) = repo.seed_files()
                {
                    send(&results, WorkerResult::IndexBatch { generation, paths });
                }
                scan_workspace(&root, |paths| {
                    if latest_index_generation.load(Ordering::Acquire) == generation {
                        send(&results, WorkerResult::IndexBatch { generation, paths });
                    }
                });
                if latest_index_generation.load(Ordering::Acquire) == generation {
                    send(&results, WorkerResult::IndexFinished { generation });
                }
            }
            WorkerCommand::SeedIndex { generation, repo } => {
                if let Ok(paths) = repo.seed_files() {
                    send(&results, WorkerResult::IndexBatch { generation, paths });
                }
            }
            WorkerCommand::Shutdown => break,
        }
    }
}

fn send(sender: &Sender<WorkerResult>, result: WorkerResult) {
    let _ = sender.send(result);
}
