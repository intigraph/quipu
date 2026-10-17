//! Produce an isolated first-use fixture for native/browser acceptance.
use quipu::{
    Store,
    share::{PointerTarget, ShareDestination, ShareOptions, SharePointer},
};
use std::{collections::BTreeMap, path::Path};

fn archive(facts: &str, pointers: Vec<SharePointer>) -> Vec<u8> {
    let mut store = Store::open_in_memory().unwrap();
    let catalogue = store.overlay_create("urn:test:catalogue", 0).unwrap();
    quipu::rdf::ingest_rdf_to_graph(
        &mut store,
        include_bytes!("../tests/fixtures/share-catalogue.ttl").as_slice(),
        oxrdfio::RdfFormat::Turtle,
        None,
        "2026-10-17T00:00:00Z",
        None,
        Some("test-catalogue"),
        catalogue,
    )
    .unwrap();
    quipu::rdf::ingest_rdf(
        &mut store,
        facts.as_bytes(),
        oxrdfio::RdfFormat::NTriples,
        None,
        "2026-10-17T00:00:00Z",
        None,
        None,
    )
    .unwrap();
    let payload = quipu::share::share_payload(
        &store,
        &ShareOptions {
            no_shapes: true,
            destination: ShareDestination::Outward,
            pointers,
            ..Default::default()
        },
        quipu::share::SHARE_PAYLOAD_MAX_BYTES,
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

fn main() {
    let args: Vec<_> = std::env::args().collect();
    assert_eq!(args.len(), 3, "pointer_fixture OUTPUT_DIR TARGET_URL");
    let dir = Path::new(&args[1]);
    std::fs::create_dir_all(dir).unwrap();
    let target = archive("<urn:remote:a> <urn:p> \"loaded\" .", vec![]);
    let pointer = SharePointer {
        graph: "urn:remote".into(),
        target: PointerTarget::Pack {
            url: args[2].clone(),
            sha256: {
                use sha2::Digest;
                format!("sha256:{}", hex::encode(sha2::Sha256::digest(&target)))
            },
        },
    };
    let declaration = archive("", vec![pointer]);
    let authority = archive("", vec![]);
    for (name, bytes) in BTreeMap::from([
        ("target.tar", target),
        ("declaration.tar", declaration),
        ("authority.tar", authority),
    ]) {
        std::fs::write(dir.join(name), bytes).unwrap();
    }
}
