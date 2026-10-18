//! Explicit offline performance experiment; no network or production store.
use crate::{
    namespace,
    store::{Datum, Store},
    types::{Op, Value},
};
fn rss_kib() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find(|l| l.starts_with("VmRSS:"))
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap()
}
#[test]
#[ignore = "explicit private 268k-label timing/RSS experiment"]
fn large_label_fixture() {
    if let Ok(path) = std::env::var("QYTJUG_PRIVATE_FIXTURE") {
        let child_dir = tempfile::tempdir().unwrap();
        let child_path = child_dir.path().join("child.db");
        std::fs::copy(path, &child_path).unwrap();
        let path = child_path.to_str().unwrap();
        let mut store = Store::open_read_only(path).unwrap();
        let cached = std::env::var("QYTJUG_PRIVATE_CACHE").unwrap() == "true";
        store.resolution_config_mut().cache_labels = cached;
        let before = rss_kib();
        let warm = std::time::Instant::now();
        if cached {
            assert!(store.prepare_resolution_index().unwrap());
        }
        let warm_ms = warm.elapsed().as_secs_f64() * 1000.;
        let after = rss_kib();
        let mut ms = Vec::new();
        let mut responses = Vec::new();
        for _ in 0..11 {
            let t = std::time::Instant::now();
            responses.push(crate::tool_resolve_entity(&store,&serde_json::json!({"name":"quipu-server memory recycle","threshold":0.85,"top_k":3})).unwrap());
            ms.push(t.elapsed().as_secs_f64() * 1000.);
        }
        assert!(responses.iter().all(|r| r == &responses[0]));
        ms.sort_by(f64::total_cmp);
        println!(
            "QYTJUG_PERF {}",
            serde_json::json!({"cache":cached,"fixture_labels":268000,"provider":"none","synthetic":true,"rss_before_kib":before,"rss_after_warm_kib":after,"rss_after_calls_kib":rss_kib(),"warm_ms":warm_ms,"mean_ms":ms.iter().sum::<f64>()/ms.len() as f64,"p50_ms":ms[5],"p95_nearest_rank_ms":ms[10],"response":responses[0]})
        );
        {
            let mut writer = Store::open(path).unwrap();
            writer.adopt_read_config_from(&store);
            let episode = serde_json::from_value(serde_json::json!({
                "name":"private-one-node-comparison",
                "source":"private-performance-fixture",
                "nodes":[{"name":"PrivateOneNodeComparison","type":"Observation"}],
                "edges":[]
            }))
            .unwrap();
            let opts = crate::episode::IngestResolutionOpts {
                enabled: true,
                threshold: 0.85,
                top_k: 3,
                strict_mode: false,
            };
            let start = std::time::Instant::now();
            let result = crate::episode::ingest_episode_with_resolution(
                &mut writer,
                &episode,
                "2026-01-01T00:00:00Z",
                namespace::DEFAULT_BASE_NS,
                Some(&opts),
            )
            .unwrap();
            println!(
                "QYTJUG_EPISODE {}",
                serde_json::json!({
                    "cache":cached,"synthetic":true,"same_body":true,
                    "elapsed_ms":start.elapsed().as_secs_f64()*1000.,
                    "hints":result.resolution_hints.len(),"outcome":format!("{:?}",result.outcome),
                })
            );
        }
        if cached {
            let mut writer = Store::open(path).unwrap();
            writer.adopt_read_config_from(&store);
            let before_updates = rss_kib();
            let mut held = Vec::new();
            let mut write_ms = Vec::new();
            for i in 0..8 {
                held.push(super::LabelIndex::build(&store).unwrap());
                let e = writer.intern("urn:synthetic:0").unwrap();
                let a = writer.intern(&format!("{}label", namespace::RDFS)).unwrap();
                let t = std::time::Instant::now();
                writer
                    .transact(
                        &[Datum {
                            entity: e,
                            attribute: a,
                            value: Value::Str(format!("Private changed label {i}")),
                            valid_from: "2026-01-01".into(),
                            valid_to: None,
                            op: Op::Assert,
                        }],
                        "2026-01-01",
                        None,
                        Some("private-held-generation-test"),
                    )
                    .unwrap();
                write_ms.push(t.elapsed().as_secs_f64() * 1000.);
                assert_eq!(
                    super::cache::resident_generation(&writer),
                    Some(writer.latest_tx_id().unwrap())
                );
            }
            let held_rss = rss_kib();
            drop(held);
            println!(
                "QYTJUG_RETAINED {}",
                serde_json::json!({"held_generations":8,"rss_before_updates_kib":before_updates,"rss_held_kib":held_rss,"rss_after_release_kib":rss_kib(),"writes_ms":write_ms})
            );
        }
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fixture.db");
    {
        let mut store = Store::open(path.to_str().unwrap()).unwrap();
        store.conn.execute_batch("BEGIN").unwrap();
        let label = store.intern(&format!("{}label", namespace::RDFS)).unwrap();
        let mut datums = Vec::with_capacity(268000);
        for i in 0..268000 {
            let e = store.intern(&format!("urn:synthetic:{i}")).unwrap();
            datums.push(Datum {
                entity: e,
                attribute: label,
                value: Value::Str(format!("Synthetic fixture label {i}")),
                valid_from: "2026-01-01".into(),
                valid_to: None,
                op: Op::Assert,
            });
        }
        store
            .transact(&datums, "2026-01-01", None, Some("private-perf-fixture"))
            .unwrap();
        store.conn.execute_batch("COMMIT").unwrap();
    }
    // Fresh children avoid attributing fixture construction's allocator pages
    // to cache RSS. Latency uses read-only handles; the cached child then
    // measures eight held generations using a separate private writer.
    for cached in [false, true] {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "resolution::perf_tests::large_label_fixture",
                "--nocapture",
            ])
            .env("QYTJUG_PRIVATE_FIXTURE", &path)
            .env("QYTJUG_PRIVATE_CACHE", cached.to_string())
            .output()
            .unwrap();
        print!("{}", String::from_utf8_lossy(&output.stdout));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
