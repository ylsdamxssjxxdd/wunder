//! Release-mode throughput probe for the stress-thread generator.
//!
//! Usage:
//!   cargo run --release -p wunder-runtime --features sqlite-storage \
//!     --example stress_bench -- [user_rounds] [model_rounds_per_turn]
//!
//! Prints wall-clock, item/tool counts and derived throughput so the
//! debug-vs-release factor and the CPU/DB split can be quantified.

use std::time::Instant;

use wunder_server::storage::{SqliteStorage, StressThreadSpec};

fn run(user_rounds: i64, model_rounds: i64) {
    let dir = std::env::temp_dir().join(format!(
        "wunder-stress-bench-{}-{}",
        std::process::id(),
        user_rounds
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("wunder.db");
    let storage = SqliteStorage::new(db_path.to_string_lossy().to_string());

    let spec = StressThreadSpec {
        session_id: format!("bench-{}-{}", user_rounds, model_rounds),
        title: format!("bench {}x{}", user_rounds, model_rounds),
        user_rounds,
        model_rounds_per_turn: model_rounds,
    };

    let t0 = Instant::now();
    let mut last = 0i64;
    let stats = storage
        .generate_stress_thread("bench", &spec, |done, _| {
            last = done;
        })
        .unwrap();
    let elapsed = t0.elapsed();
    assert_eq!(last, user_rounds);

    let secs = elapsed.as_secs_f64();
    println!(
        "RESULT turns={} model_rounds={} items={} tool_calls={} elapsed={:.3}s items_per_s={:.1} tool_per_s={:.1} model_rounds_per_s={:.1}",
        user_rounds,
        model_rounds,
        stats.items,
        stats.tool_calls,
        secs,
        stats.items as f64 / secs,
        stats.tool_calls as f64 / secs,
        (user_rounds * model_rounds) as f64 / secs,
    );

    let _ = std::fs::remove_dir_all(&dir);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let user_rounds: i64 = args.get(1).and_then(|v| v.parse().ok()).unwrap_or(50);
    let model_rounds: i64 = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(20);
    println!("profile: sqlite-storage, {:?}", if cfg!(debug_assertions) { "DEBUG" } else { "RELEASE" });
    run(user_rounds, model_rounds);
    // Second, larger run to amortise warm-up.
    if user_rounds >= 50 {
        run((user_rounds * 4).min(1000), model_rounds);
    }
}
