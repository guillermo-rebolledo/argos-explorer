use std::path::{Path, PathBuf};

use crossbeam_channel::{Receiver, Sender, bounded};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use thiserror::Error;

#[derive(Debug, Clone)]
pub enum WatchNotice {
    Changed(Vec<PathBuf>),
    Error(String),
}

#[derive(Debug, Error)]
pub enum WatchError {
    #[error("could not initialize filesystem watcher: {0}")]
    Initialize(#[source] notify::Error),
    #[error("could not watch Workspace Root {path}: {source}")]
    Watch {
        path: PathBuf,
        #[source]
        source: notify::Error,
    },
}

pub struct WatchService {
    _watcher: RecommendedWatcher,
    receiver: Receiver<WatchNotice>,
}

impl WatchService {
    pub fn start(root: &Path) -> Result<Self, WatchError> {
        let (sender, receiver) = bounded(4096);
        let handler_sender = sender.clone();
        let mut watcher = notify::recommended_watcher(move |event| {
            handle_event(&handler_sender, event);
        })
        .map_err(WatchError::Initialize)?;
        watcher
            .watch(root, RecursiveMode::Recursive)
            .map_err(|source| WatchError::Watch {
                path: root.to_path_buf(),
                source,
            })?;
        Ok(Self {
            _watcher: watcher,
            receiver,
        })
    }

    pub fn try_iter(&self) -> impl Iterator<Item = WatchNotice> + '_ {
        self.receiver.try_iter()
    }
}

fn handle_event(sender: &Sender<WatchNotice>, event: notify::Result<Event>) {
    let notice = match event {
        Ok(event) if should_invalidate(&event.kind) => WatchNotice::Changed(event.paths),
        Ok(_) => return,
        Err(error) => WatchNotice::Error(error.to_string()),
    };
    let _ = sender.try_send(notice);
}

fn should_invalidate(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::event::{AccessKind, ModifyKind};

    #[test]
    fn file_reads_do_not_mark_previews_stale() {
        assert!(!should_invalidate(&EventKind::Access(AccessKind::Any)));
        assert!(should_invalidate(&EventKind::Modify(ModifyKind::Any)));
    }
}
