//! `quipu follow` — keep this database a read-only follower of a primary
//! (aegis-tnh). The contract lives on `quipu::follow`; this is the run loop.

use std::time::Duration;

use crate::cli::flag_value;
use quipu::follow::{Follower, Halt, Pace};

/// Longest wait between reconnect attempts.
const MAX_BACKOFF: Duration = Duration::from_secs(60);

fn count(args: &[String], name: &str, default: u64) -> u64 {
    flag_value(args, name).map_or(default, |s| {
        s.parse().unwrap_or_else(|_| {
            eprintln!("{name} must be a non-negative integer, got {s:?}");
            std::process::exit(2);
        })
    })
}

/// `quipu follow --primary <url> [--page-txs N] [--page-gap-ms N]
/// [--bootstrap-only] [--db <path>]`. `--page-txs` is at most 250 and
/// `--page-gap-ms` at least 1000.
///
/// Pages the primary's change feed from this database's cursor until it is
/// current, then tails the change stream for as long as it runs. Exit 0 after
/// `--bootstrap-only`; 1 on a local store error; 3 when the follower must be
/// rebuilt (another source, a digest mismatch, a refused record).
pub fn cmd_follow(args: &[String], db_path: &str) {
    let Some(primary) = flag_value(args, "--primary") else {
        eprintln!("follow needs --primary <url>");
        std::process::exit(2);
    };
    // The quipu steward's ceiling (aegis-tnh): at most 250 transactions a
    // page and one page a second, so a bootstrap cannot push the primary
    // toward its memory recycle.
    let pace = Pace {
        page_txs: usize::try_from(count(args, "--page-txs", 200).clamp(1, 250)).unwrap_or(200),
        page_gap_ms: count(args, "--page-gap-ms", 1_000).max(1_000),
    };
    let mut store = quipu::Store::open(db_path).unwrap_or_else(|e| {
        eprintln!("error opening follower {db_path}: {e}");
        std::process::exit(1);
    });
    let feed = quipu::follow_http::Http::new(primary);
    match feed.frozen_graphs() {
        Ok(frozen) if frozen.is_empty() => eprintln!("follow: the primary has no frozen graphs"),
        Ok(frozen) => eprintln!(
            "follow: {} graph(s) frozen into packs are NOT followed: {}",
            frozen.len(),
            frozen.join(" ")
        ),
        Err(error) => eprintln!("follow: frozen-graph count UNKNOWN: {error}"),
    }
    let mut follower = Follower::open(&mut store, feed).unwrap_or_else(|e| {
        eprintln!("error opening follower progress: {e}");
        std::process::exit(1);
    });
    let mut backoff = Duration::from_secs(1);
    let mut sleep = |ms: u64| std::thread::sleep(Duration::from_millis(ms));
    loop {
        match follower.bootstrap(pace, &mut sleep) {
            Ok(pages) => {
                eprintln!(
                    "follow: current at source tx {} ({pages} pages)",
                    follower.cursor()
                );
                break;
            }
            Err(halt) => backoff = handle(halt, follower.cursor(), backoff),
        }
    }
    if args.iter().any(|a| a == "--bootstrap-only") {
        return;
    }
    loop {
        match follower.tail_once(None) {
            Ok(()) => backoff = Duration::from_secs(1),
            Err(halt) => backoff = handle(halt, follower.cursor(), backoff),
        }
    }
}

/// Exit on a store error or a refusal; otherwise wait and return the next wait.
fn handle(halt: Halt, cursor: i64, backoff: Duration) -> Duration {
    match halt {
        Halt::Transport(error) => {
            eprintln!("follow: primary unavailable at tx {cursor}: {error}; retry in {backoff:?}");
            std::thread::sleep(backoff);
            (backoff * 2).min(MAX_BACKOFF)
        }
        Halt::Store(error) => {
            eprintln!("follow: local store error at tx {cursor}: {error}");
            std::process::exit(1);
        }
        Halt::Rebuild(reason) => {
            eprintln!("follow: STOPPED at tx {cursor}, rebuild the follower: {reason}");
            std::process::exit(3);
        }
        Halt::Budget => backoff,
        Halt::TooLarge(reason) => {
            eprintln!("follow: STOPPED at tx {cursor}: {reason}");
            std::process::exit(3);
        }
    }
}
