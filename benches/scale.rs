use std::{
    env,
    fs::{self, OpenOptions},
    io::{Seek, SeekFrom, Write},
    path::PathBuf,
    time::{Duration, Instant},
};

use argos_explorer::{search::QuickOpen, viewer::search_large_file, workspace::load_directory};
use criterion::{BatchSize, Criterion, Throughput, criterion_group, criterion_main};

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn quick_open_index(c: &mut Criterion) {
    let count = env_usize("ARGOS_EXPLORER_SCALE_PATHS", 100_000);
    let root = PathBuf::from("C:/scale-workspace");
    let paths: Vec<_> = (0..count)
        .map(|index| root.join(format!("package-{}/src/module-{index}.rs", index % 1000)))
        .collect();
    let mut group = c.benchmark_group("quick_open");
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(5));
    group.throughput(Throughput::Elements(count as u64));
    group.bench_function(format!("index_and_publish_{count}"), |benchmark| {
        benchmark.iter_batched(
            || QuickOpen::new(512 * 1024 * 1024),
            |mut finder| {
                finder.add_paths(&root, paths.clone());
                let deadline = Instant::now() + Duration::from_secs(60);
                while finder.result_count() < count && Instant::now() < deadline {
                    finder.tick();
                    std::hint::spin_loop();
                }
                assert_eq!(finder.result_count(), count);
            },
            BatchSize::LargeInput,
        );
    });
    group.finish();
}

fn large_directory(c: &mut Criterion) {
    let count = env_usize("ARGOS_EXPLORER_SCALE_DIR_ENTRIES", 2_000);
    let temp = tempfile::tempdir().unwrap();
    for index in 0..count {
        fs::write(temp.path().join(format!("file-{index}.txt")), b"x").unwrap();
    }
    let root = fs::canonicalize(temp.path()).unwrap();
    let mut group = c.benchmark_group("workspace_tree");
    group.sample_size(10);
    group.throughput(Throughput::Elements(count as u64));
    group.bench_function(format!("enumerate_and_sort_{count}"), |benchmark| {
        benchmark.iter(|| {
            let listing = load_directory(&root, &root, false);
            assert_eq!(listing.entries.len(), count);
        });
    });
    group.finish();
}

fn large_file_search(c: &mut Criterion) {
    let size_mib = env_usize("ARGOS_EXPLORER_SCALE_FILE_MIB", 64);
    let size = size_mib as u64 * 1024 * 1024;
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.as_file_mut().set_len(size).unwrap();
    file.seek(SeekFrom::End(-16)).unwrap();
    file.write_all(b"needle-at-end\n").unwrap();
    file.flush().unwrap();
    let path = file.path().to_path_buf();
    let mut group = c.benchmark_group("large_file");
    group.sample_size(10);
    group.throughput(Throughput::Bytes(size));
    group.bench_function(format!("stream_search_{size_mib}_mib"), |benchmark| {
        benchmark.iter(|| {
            let matches = search_large_file(&path, "needle-at-end").unwrap();
            assert_eq!(matches.len(), 1);
        });
    });
    group.finish();

    // Exercise ordinary path creation as part of the packaging benchmark build.
    let _ = OpenOptions::new().read(true).open(path).unwrap();
}

criterion_group!(
    benches,
    quick_open_index,
    large_directory,
    large_file_search
);
criterion_main!(benches);
