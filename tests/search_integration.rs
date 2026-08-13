use std::{
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use argos_explorer::search::{QuickOpen, fuzzy_score};

fn wait_for_matches(finder: &mut QuickOpen, minimum: usize) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        finder.tick();
        if finder.result_count() >= minimum {
            return;
        }
        thread::sleep(Duration::from_millis(5));
    }
    panic!("matcher did not produce {minimum} results");
}

#[test]
fn fuzzy_path_query_ranks_matching_files() {
    let root = PathBuf::from("C:/workspace");
    let mut finder = QuickOpen::new(1024 * 1024);
    finder.add_paths(
        &root,
        vec![
            root.join("src/workspace.rs"),
            root.join("src/worker.rs"),
            root.join("docs/design.md"),
        ],
    );
    finder.set_query("wrksp".to_owned());
    wait_for_matches(&mut finder, 1);

    assert!(
        finder
            .selected_record()
            .unwrap()
            .path
            .ends_with("src/workspace.rs")
    );
}

#[test]
fn memory_budget_degrades_to_disclosed_partial_index() {
    let root = PathBuf::from("C:/workspace");
    let mut finder = QuickOpen::new(1);
    finder.add_paths(&root, vec![root.join("large-name.rs")]);

    assert!(finder.is_partial());
    assert_eq!(finder.indexed_count(), 0);
}

#[test]
fn local_filters_match_noncontiguous_characters() {
    assert!(fuzzy_score("src/workspace.rs", "wrksp").is_some());
    assert!(fuzzy_score("src/worker.rs", "wrksp").is_none());
}
