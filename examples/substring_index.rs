//! Explicit bounded substring-index preparation on a quiescent owned copy.
//! For operator-controlled single batches, use `quipu search-index backfill
//! --kind substring --batch-size 10000 --db COPY` instead. Never run this on
//! an active service database or a signed pack: preparation changes the file.
use std::time::{Duration, Instant};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 || args[2] != "--provision" {
        return Err("usage: substring_index OWNED_COPY --provision".into());
    }
    let store = quipu::Store::open(&args[1])?;
    let started = Instant::now();
    for batch in 0..100 {
        if started.elapsed() > Duration::from_secs(170) {
            return Err(
                "preparation deadline exceeded; retain progress for explicit resume".into(),
            );
        }
        let progress = store.backfill_substring_batch(10_000)?;
        if progress.complete {
            if !store.substring_index_ready()? {
                return Err("complete index does not have the supported schema".into());
            }
            println!(
                "{}",
                serde_json::json!({"complete":true,"batches":batch+1,
                "elapsed_seconds":started.elapsed().as_secs_f64(),"progress":progress})
            );
            return Ok(());
        }
    }
    Err("batch budget exhausted; retain progress for explicit resume".into())
}
