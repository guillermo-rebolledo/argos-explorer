use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use argos_explorer::search::QuickOpen;

fn main() {
    let count = std::env::args()
        .nth(1)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(1_000_000);
    let root = PathBuf::from("C:/scale-workspace");
    let mut finder = QuickOpen::new(512 * 1024 * 1024);
    let started = Instant::now();
    for batch_start in (0..count).step_by(4096) {
        let batch_end = batch_start.saturating_add(4096).min(count);
        let paths = (batch_start..batch_end)
            .map(|index| {
                root.join(format!(
                    "packages/{:06}/src/{:064}/module-{index:07}.rs",
                    index % 10_000,
                    index
                ))
            })
            .collect();
        finder.add_paths(&root, paths);
        if finder.is_partial() {
            break;
        }
    }

    let deadline = Instant::now() + Duration::from_secs(60);
    while finder.result_count() < finder.indexed_count() && Instant::now() < deadline {
        finder.tick();
        std::hint::spin_loop();
    }
    let elapsed = started.elapsed();
    let peak = peak_working_set_bytes();
    println!("indexed_paths={}", finder.indexed_count());
    println!("elapsed_ms={}", elapsed.as_millis());
    println!(
        "estimated_index_mib={:.1}",
        finder.estimated_memory_bytes() as f64 / 1_048_576.0
    );
    if let Some(peak) = peak {
        println!("peak_process_mib={:.1}", peak as f64 / 1_048_576.0);
    }
    println!("partial={}", finder.is_partial());

    assert_eq!(
        finder.indexed_count(),
        count,
        "the accepted path count must fit"
    );
    assert!(!finder.is_partial(), "the accepted index must be complete");
    assert_eq!(
        finder.result_count(),
        count,
        "all paths must reach the matcher snapshot"
    );
}

#[cfg(windows)]
fn peak_working_set_bytes() -> Option<usize> {
    use std::mem::{size_of, zeroed};
    use windows_sys::Win32::System::{
        ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS},
        Threading::GetCurrentProcess,
    };

    let mut counters: PROCESS_MEMORY_COUNTERS = unsafe { zeroed() };
    counters.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
    let success = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
    };
    (success != 0).then_some(counters.PeakWorkingSetSize)
}

#[cfg(not(windows))]
fn peak_working_set_bytes() -> Option<usize> {
    None
}
