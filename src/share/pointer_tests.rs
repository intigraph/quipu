//! Pendant pointer entries in the share manifest (aegis-3fy1kp).
use super::*;

const SHA: &str = "4f2d1c0b9a8e7d6c5b4a39281706f5e4d3c2b1a0f9e8d7c6b5a4938271605f4e";

fn store_with(facts: &str) -> Store {
    let mut store = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut store);
    if !facts.is_empty() {
        crate::rdf::ingest_rdf(
            &mut store,
            facts.as_bytes(),
            oxrdfio::RdfFormat::NTriples,
            None,
            "2026-10-15T00:00:00Z",
            None,
            None,
        )
        .unwrap();
    }
    store
}

fn endpoint(graph: &str, url: &str) -> SharePointer {
    SharePointer {
        graph: graph.into(),
        target: PointerTarget::Endpoint {
            endpoint: url.into(),
        },
    }
}

fn pack(graph: &str, url: &str, sha: &str) -> SharePointer {
    SharePointer {
        graph: graph.into(),
        target: PointerTarget::Pack {
            url: url.into(),
            sha256: sha.into(),
        },
    }
}

fn opts(pointers: Vec<SharePointer>) -> ShareOptions {
    ShareOptions {
        no_shapes: true,
        destination: ShareDestination::Internal,
        pointers,
        ..Default::default()
    }
}

/// Shares into `<dir>/pack` (share refuses an existing destination) and returns
/// that directory with the manifest.
fn produce(store: &Store, dir: &std::path::Path, pointers: Vec<SharePointer>) -> ShareManifest {
    share(store, dir.join("pack").to_str().unwrap(), &opts(pointers)).unwrap()
}

fn pack_dir(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("pack")
}

#[test]
fn a_pointer_free_share_is_byte_identical_to_before() {
    let store = store_with("<urn:a> <urn:p> \"x\" .\n");
    let dir = tempfile::tempdir().unwrap();
    produce(&store, dir.path(), Vec::new());
    let json = std::fs::read_to_string(pack_dir(dir.path()).join("manifest.json")).unwrap();
    assert!(!json.contains("pointers"), "{json}");
}

#[test]
fn pointers_are_inside_the_share_identity() {
    let store = store_with("<urn:a> <urn:p> \"x\" .\n");
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let plain = produce(&store, a.path(), Vec::new());
    let pointed = produce(
        &store,
        b.path(),
        vec![endpoint(
            "https://example.org/homelab",
            "https://quipu.example.org/sparql",
        )],
    );
    assert_ne!(plain.share_id, pointed.share_id, "a pointer changes the id");
    assert_eq!(plain.graph_hash, pointed.graph_hash, "...but not the graph");

    // Stripping the pointer from the published manifest must not verify.
    let path = pack_dir(b.path()).join("manifest.json");
    let mut value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("pointers");
    std::fs::write(&path, serde_json::to_string(&value).unwrap()).unwrap();
    let mut request =
        crate::share_transport::read_local(pack_dir(b.path()).to_str().unwrap()).unwrap();
    request.destination = ShareDestination::Internal;
    let err = crate::share_import::verify_share(&request)
        .unwrap_err()
        .to_string();
    assert!(err.contains("share id mismatch"), "{err}");
}

#[test]
fn malformed_or_credential_bearing_pointers_are_refused_by_the_producer() {
    let store = store_with("<urn:a> <urn:p> \"x\" .\n");
    let bad = [
        (
            vec![endpoint("not an iri", "https://q.example.org/sparql")],
            "not an IRI",
        ),
        (
            vec![endpoint("urn:g", "ftp://q.example.org/sparql")],
            "not an http(s) URL",
        ),
        (
            vec![endpoint("urn:g", "https://user:pw@q.example.org/sparql")],
            "credentials",
        ),
        (
            vec![endpoint(
                "urn:g",
                "https://q.example.org/sparql?access_token=s3cret",
            )],
            "query or fragment",
        ),
        (
            vec![pack("urn:g", "https://example.org/p.tar.gz#frag", SHA)],
            "query or fragment",
        ),
        (
            vec![pack("urn:g", "https://example.org/p.pendant.tar.gz", "abc")],
            "64 lowercase hex",
        ),
        (
            vec![
                endpoint("urn:g", "https://a.example.org/sparql"),
                pack("urn:g", "https://example.org/p.tar.gz", SHA),
            ],
            "declared twice",
        ),
    ];
    for (pointers, needle) in bad {
        let dir = tempfile::tempdir().unwrap();
        let err = share(
            &store,
            pack_dir(dir.path()).to_str().unwrap(),
            &opts(pointers),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains(needle), "{needle}: {err}");
    }
}

#[test]
fn a_hand_edited_manifest_cannot_smuggle_what_the_producer_refuses() {
    // Re-hash after editing, so the id check alone would pass: the verifier must
    // apply the producer's own pointer rules.
    let store = store_with("<urn:a> <urn:p> \"x\" .\n");
    let dir = tempfile::tempdir().unwrap();
    let mut manifest = produce(
        &store,
        dir.path(),
        vec![endpoint("urn:g", "https://q.example.org/sparql")],
    );
    manifest.pointers = vec![endpoint("urn:g", "https://user:pw@q.example.org/sparql")];
    manifest.share_id = sha256(&manifest_bytes(&manifest, false).unwrap());
    std::fs::write(
        pack_dir(dir.path()).join("manifest.json"),
        manifest_bytes(&manifest, true).unwrap(),
    )
    .unwrap();
    let mut request =
        crate::share_transport::read_local(pack_dir(dir.path()).to_str().unwrap()).unwrap();
    request.destination = ShareDestination::Internal;
    let err = crate::share_import::verify_share(&request)
        .unwrap_err()
        .to_string();
    assert!(err.contains("credentials"), "{err}");
}

#[cfg(feature = "shacl")]
#[test]
fn a_pointer_only_share_carries_no_facts_and_composition_reports_without_following() {
    let store = store_with("");
    let dir = tempfile::tempdir().unwrap();
    let pointers = vec![
        endpoint(
            "https://example.org/homelab",
            "https://quipu.example.org/sparql",
        ),
        pack(
            "https://example.org/reference",
            "https://example.org/ref.pendant.tar.gz",
            SHA,
        ),
    ];
    let manifest = produce(&store, dir.path(), pointers.clone());
    assert_eq!(
        std::fs::read_to_string(pack_dir(dir.path()).join("export.nt")).unwrap(),
        "",
        "a repository that ships only pointers ships an empty pack"
    );
    let mut request =
        crate::share_transport::read_local(pack_dir(dir.path()).to_str().unwrap()).unwrap();
    request.destination = ShareDestination::Internal;
    let mut target = Store::open_in_memory().unwrap();
    let composition =
        crate::share_compose::compose(&mut target, &[request], None, "2026-10-15T00:00:00Z", None)
            .unwrap();
    assert_eq!(composition.pointers.len(), 2);
    assert!(
        composition
            .pointers
            .iter()
            .all(|p| p.share_id == manifest.share_id)
    );
    assert_eq!(
        composition
            .pointers
            .iter()
            .map(|p| p.pointer.clone())
            .collect::<Vec<_>>(),
        pointers
    );
    assert!(
        target
            .lookup("https://example.org/homelab")
            .unwrap()
            .is_none()
            && target
                .lookup("https://example.org/reference")
                .unwrap()
                .is_none(),
        "pointers are reported, never followed or registered"
    );
}

fn outward(pointers: Vec<SharePointer>) -> ShareOptions {
    ShareOptions {
        no_shapes: true,
        pointers,
        ..Default::default()
    }
}

#[test]
fn an_outward_share_scrubs_pointer_urls_like_facts() {
    // CONTROL: the same identifier in facts is refused outward.
    let leaky = store_with("<urn:a> <urn:p> \"FIXTURE_PRIVATE_TOKEN\" .\n");
    let dir = tempfile::tempdir().unwrap();
    let err = share(
        &leaky,
        pack_dir(dir.path()).to_str().unwrap(),
        &outward(Vec::new()),
    )
    .unwrap_err()
    .to_string();
    assert!(err.contains("export.nt"), "{err}");

    let store = store_with("<urn:a> <urn:p> \"x\" .\n");
    for pointer in [
        endpoint("urn:g", "http://FIXTURE_PRIVATE_TOKEN.example/sparql"),
        pack(
            "urn:g",
            "https://example.org/FIXTURE_PRIVATE_TOKEN.pendant.tar.gz",
            SHA,
        ),
        endpoint("urn:FIXTURE_PRIVATE_TOKEN", "https://q.example.org/sparql"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let err = share(
            &store,
            pack_dir(dir.path()).to_str().unwrap(),
            &outward(vec![pointer]),
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("manifest.json pointers"), "{err}");
        assert!(!pack_dir(dir.path()).join("manifest.json").exists());
    }

    // The same pointer is fine when the share is explicitly internal.
    let dir = tempfile::tempdir().unwrap();
    produce(
        &store,
        dir.path(),
        vec![endpoint(
            "urn:g",
            "http://FIXTURE_PRIVATE_TOKEN.example/sparql",
        )],
    );
}
