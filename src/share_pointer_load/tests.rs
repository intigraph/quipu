use super::*;
use crate::share::{ShareDestination, ShareOptions};
use crate::share_transport::read_archive_bytes;

const TS: &str = "2026-10-17T00:00:00Z";

fn archive(facts: &str, pointers: Vec<SharePointer>) -> Vec<u8> {
    let mut store = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    crate::rdf::ingest_rdf(
        &mut store,
        facts.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        TS,
        None,
        None,
    )
    .unwrap();
    let payload = crate::share::share_payload(
        &store,
        &ShareOptions {
            no_shapes: true,
            destination: ShareDestination::Internal,
            pointers,
            ..Default::default()
        },
        crate::share::SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap();
    let mut tar = tar::Builder::new(Vec::new());
    for (name, bytes) in payload.files {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        tar.append_data(&mut header, name, bytes.as_bytes())
            .unwrap();
    }
    tar.into_inner().unwrap()
}

fn request(bytes: &[u8]) -> ShareImportRequest {
    let mut r = read_archive_bytes(bytes, "test", false).unwrap();
    r.destination = ShareDestination::Internal;
    r
}

fn loader(bytes: &[u8]) -> LazyPacks {
    let pointer = SharePointer {
        graph: "urn:remote".into(),
        target: PointerTarget::Pack {
            url: "https://example.org/remote.pendant.tar".into(),
            sha256: crate::share::sha256(bytes),
        },
    };
    let unused = SharePointer {
        graph: "urn:unused".into(),
        target: PointerTarget::Pack {
            url: "https://example.org/never-fetched.tar".into(),
            sha256: "0".repeat(64),
        },
    };
    let decl = request(&archive("", vec![pointer, unused]));
    let authority = request(&archive("", vec![]));
    LazyPacks::new(decl, authority).unwrap()
}

fn fail(message: &str) -> Error {
    Error::InvalidValue(message.into())
}

#[test]
fn first_use_fetches_only_requested_graph_and_cached_use_is_offline() {
    let bytes = archive("<urn:remote:a> <urn:p> \"loaded\" .", vec![]);
    let loader = loader(&bytes); // no callback, no I/O at declaration
    let mut store = Store::open_in_memory().unwrap();
    let mut cache = BTreeMap::new();
    assert!(store.dataset_list().unwrap().is_empty());
    let mut calls = 0;
    let first = loader
        .use_graph(
            &mut store,
            "urn:remote",
            &mut cache,
            |_, _| Ok(()),
            |_| {
                calls += 1;
                Ok(bytes.clone())
            },
            TS,
            None,
        )
        .unwrap();
    assert_eq!(calls, 1);
    assert_eq!(cache.len(), 1);
    let rows = crate::sparql_query(
        &store,
        &format!(
            "SELECT ?v FROM <{}> WHERE {{ <urn:remote:a> <urn:p> ?v }}",
            first.dataset
        ),
    )
    .unwrap();
    assert_eq!(rows.rows().len(), 1);
    assert!(store.current_facts().unwrap().is_empty());
    assert!(store.list_shapes().unwrap().is_empty());
    let again = loader
        .use_graph(
            &mut store,
            "urn:remote",
            &mut cache,
            |_, _| Ok(()),
            |_| Err(fail("offline")),
            TS,
            None,
        )
        .unwrap();
    assert_eq!(first.dataset, again.dataset);
}

#[test]
fn trust_is_checked_before_network_and_before_cached_reuse() {
    let bytes = archive("<urn:a> <urn:p> \"x\" .", vec![]);
    let loader = loader(&bytes);
    let mut store = Store::open_in_memory().unwrap();
    let mut cache = BTreeMap::new();
    for cached in [false, true] {
        if cached {
            cache.insert(crate::share::sha256(&bytes), bytes.clone());
        }
        let err = loader
            .use_graph(
                &mut store,
                "urn:remote",
                &mut cache,
                |_, _| Err(fail("caboodle trust refused")),
                |_| panic!("must not fetch"),
                TS,
                None,
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("pack pointer urn:remote: caboodle trust refused"),
            "{err}"
        );
        assert!(store.dataset_list().unwrap().is_empty());
    }
}

#[test]
fn missing_graph_and_endpoint_do_not_fetch() {
    let bytes = archive("", vec![]);
    let loader = loader(&bytes);
    let mut store = Store::open_in_memory().unwrap();
    let err = loader
        .use_graph(
            &mut store,
            "urn:absent",
            &mut BTreeMap::new(),
            |_, _| panic!("no trust decision for absent graph"),
            |_| panic!("no fetch"),
            TS,
            None,
        )
        .unwrap_err();
    assert!(err.to_string().contains("urn:absent"));
    let endpoint = SharePointer {
        graph: "urn:endpoint".into(),
        target: PointerTarget::Endpoint {
            endpoint: "https://example.org/sparql".into(),
        },
    };
    let loader = LazyPacks::new(request(&archive("", vec![endpoint])), request(&bytes)).unwrap();
    assert!(
        loader
            .pointer("urn:endpoint")
            .unwrap_err()
            .to_string()
            .contains("federation")
    );
}

#[test]
fn corrupt_fetch_and_corrupt_cache_refuse_without_partial_load_or_retry() {
    let bytes = archive("<urn:a> <urn:p> \"x\" .", vec![]);
    let loader = loader(&bytes);
    for from_cache in [false, true] {
        let mut store = Store::open_in_memory().unwrap();
        let mut cache = BTreeMap::new();
        if from_cache {
            cache.insert(crate::share::sha256(&bytes), vec![0]);
        }
        let mut calls = 0;
        let err = loader
            .use_graph(
                &mut store,
                "urn:remote",
                &mut cache,
                |_, _| Ok(()),
                |_| {
                    calls += 1;
                    Ok(vec![0])
                },
                TS,
                None,
            )
            .unwrap_err();
        assert!(err.to_string().contains("archive sha256 mismatch"));
        assert!(err.to_string().contains("urn:remote"));
        assert_eq!(calls, usize::from(!from_cache));
        assert!(store.dataset_list().unwrap().is_empty());
        assert!(store.lookup("urn:a").unwrap().is_none());
    }
}

#[test]
fn network_failure_names_pointer_and_leaves_store_and_cache_empty() {
    let bytes = archive("", vec![]);
    let mut store = Store::open_in_memory().unwrap();
    let mut cache = BTreeMap::new();
    let err = loader(&bytes)
        .use_graph(
            &mut store,
            "urn:remote",
            &mut cache,
            |_, _| Ok(()),
            |_| Err(fail("HTTP 503")),
            TS,
            None,
        )
        .unwrap_err();
    assert!(err.to_string().contains("urn:remote"));
    assert!(err.to_string().contains("HTTP 503"));
    assert!(cache.is_empty());
    assert!(store.dataset_list().unwrap().is_empty());
}

#[test]
fn correctly_pinned_archive_with_forged_share_id_is_not_cached() {
    let original = archive("<urn:a> <urn:p> \"x\" .", vec![]);
    let id = request(&original).manifest.share_id;
    let replacement = format!("sha256:{}", "0".repeat(64));
    let mut forged = original.clone();
    // Equal-length replacement leaves the tar member size and header valid.
    for offset in 0..=forged.len() - id.len() {
        if &forged[offset..offset + id.len()] == id.as_bytes() {
            forged[offset..offset + id.len()].copy_from_slice(replacement.as_bytes());
        }
    }
    let mut store = Store::open_in_memory().unwrap();
    let mut cache = BTreeMap::new();
    let err = loader(&forged)
        .use_graph(
            &mut store,
            "urn:remote",
            &mut cache,
            |_, _| Ok(()),
            |_| Ok(forged.clone()),
            TS,
            None,
        )
        .unwrap_err();
    assert!(err.to_string().contains("share id mismatch"), "{err}");
    assert!(cache.is_empty());
    assert!(store.dataset_list().unwrap().is_empty());
}

#[test]
fn quarantine_rolls_back_every_graph_without_adopting_foreign_policy() {
    let bytes = archive(
        "<urn:a> <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:UntrustedType> .",
        vec![],
    );
    let mut store = Store::open_in_memory().unwrap();
    let err = loader(&bytes)
        .use_graph(
            &mut store,
            "urn:remote",
            &mut BTreeMap::new(),
            |_, _| Ok(()),
            |_| Ok(bytes.clone()),
            TS,
            None,
        )
        .unwrap_err();
    assert!(err.to_string().contains("quarantined"), "{err}");
    assert!(store.dataset_list().unwrap().is_empty());
    assert!(store.lookup("urn:a").unwrap().is_none());
    assert!(store.list_shapes().unwrap().is_empty());
}

#[test]
fn gzip_archive_and_bare_digest_use_the_same_verification_path() {
    use std::io::Write;
    let tar = archive("<urn:a> <urn:p> \"gzip\" .", vec![]);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar).unwrap();
    let bytes = encoder.finish().unwrap();
    let sha = crate::share::sha256(&bytes);
    let pointer = SharePointer {
        graph: "urn:gzip".into(),
        target: PointerTarget::Pack {
            url: "https://example.org/archive-without-extension".into(),
            sha256: sha.strip_prefix("sha256:").unwrap().into(),
        },
    };
    let declaration = request(&archive("", vec![pointer]));
    let loader = LazyPacks::new(declaration, request(&archive("", vec![]))).unwrap();
    let mut store = Store::open_in_memory().unwrap();
    let result = loader
        .use_graph(
            &mut store,
            "urn:gzip",
            &mut BTreeMap::new(),
            |_, _| Ok(()),
            |_| Ok(bytes.clone()),
            TS,
            None,
        )
        .unwrap();
    assert_eq!(result.outcome, "composed");
}
