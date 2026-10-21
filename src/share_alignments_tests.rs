use super::*;
use crate::share::{
    SHARE_PAYLOAD_MAX_BYTES, ShareDestination, ShareOptions, SharePayload, share_payload,
};
use crate::share_import::{ShareImportRequest, import_share, verify_share};

const EMPTY: &str = "@prefix sssom: <https://w3id.org/sssom/> .\n";

fn payload(member: Option<&str>) -> SharePayload {
    share_payload(
        &Store::open_in_memory().unwrap(),
        &ShareOptions {
            no_shapes: true,
            destination: ShareDestination::Internal,
            alignments_turtle: member.map(str::to_owned),
            ..Default::default()
        },
        SHARE_PAYLOAD_MAX_BYTES,
    )
    .unwrap()
}

fn request(payload: &SharePayload) -> ShareImportRequest {
    let mut r: ShareImportRequest = serde_json::from_value(serde_json::json!({
        "manifest": payload.manifest,
        "export_ntriples": payload.files["export.nt"],
        "shapes_turtle": payload.files["shapes.ttl"],
        "alignments_turtle": payload.files.get(ALIGNMENTS_FILE),
        "source": "private alignment transport fixture"
    }))
    .unwrap();
    r.destination = ShareDestination::Internal;
    r
}

#[test]
fn legacy_absence_has_no_new_wire_fields_or_member() {
    let p = payload(None);
    let value = serde_json::to_value(&p.manifest).unwrap();
    assert!(value.get("alignments_hash").is_none());
    assert!(value["files"].get("alignments").is_none());
    assert!(!p.files.contains_key(ALIGNMENTS_FILE));
    verify_share(&request(&p)).unwrap();
}

#[test]
fn exact_empty_carrier_bytes_are_part_of_identity_and_rdf_view() {
    let p = payload(Some(EMPTY));
    assert_eq!(p.manifest.alignments_hash, Some(sha256(EMPTY.as_bytes())));
    assert_eq!(
        p.manifest.files.alignments.as_deref(),
        Some(ALIGNMENTS_FILE)
    );
    assert_eq!(p.files[ALIGNMENTS_FILE], EMPTY);
    assert!(p.files["manifest.ttl"].contains("payload:alignments.ttl"));
    verify_share(&request(&p)).unwrap();
    let changed = payload(Some(&(EMPTY.to_owned() + "# different exact bytes\n")));
    assert_ne!(p.manifest.share_id, changed.manifest.share_id);
}

#[test]
fn mismatched_missing_undeclared_and_misnamed_members_refuse_before_writes() {
    let original = request(&payload(Some(EMPTY)));
    for case in 0..7 {
        let mut r = original.clone();
        match case {
            0 => r.alignments_turtle = None,
            1 => r.alignments_turtle = Some("# tampered\n".into()),
            2 => {
                r.manifest.alignments_hash = None;
                r.manifest.files.alignments = None;
            }
            3 => r.manifest.files.alignments = Some("../alignments.ttl".into()),
            4 => r.manifest.alignments_hash = None,
            5 => r.manifest.files.alignments = None,
            6 => {
                let text = "# rehashed but identity not updated\n";
                r.alignments_turtle = Some(text.into());
                r.manifest.alignments_hash = Some(sha256(text.as_bytes()));
            }
            _ => unreachable!(),
        }
        let mut receiver = Store::open_in_memory().unwrap();
        let before = receiver.latest_tx_id().unwrap();
        assert!(
            import_share(&mut receiver, &r, "2026-10-21T00:00:00Z", None).is_err(),
            "case {case}"
        );
        assert_eq!(receiver.latest_tx_id().unwrap(), before);
        assert!(
            imported_member(&receiver, &r.manifest.share_id)
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn malformed_even_resealed_and_oversized_evidence_refuses() {
    assert!(validate("<broken").is_err());
    assert!(validate(&" ".repeat(MAX_ALIGNMENT_BYTES + 1)).is_err());
    let mut r = request(&payload(Some(EMPTY)));
    let bad = "<broken";
    r.alignments_turtle = Some(bad.into());
    r.manifest.alignments_hash = Some(sha256(bad.as_bytes()));
    r.manifest.share_id = sha256(&crate::share::manifest_bytes(&r.manifest, false).unwrap());
    assert!(verify_share(&r).is_err());
}

#[test]
fn nonempty_records_are_retained_without_loading_identity_or_shape_authority() {
    let text =
        "<urn:alignment:left> <http://www.w3.org/2002/07/owl#sameAs> <urn:alignment:right> .\n";
    let r = request(&payload(Some(text)));
    let mut receiver = Store::open_in_memory().unwrap();
    import_share(&mut receiver, &r, "2026-10-21T00:00:00Z", None).unwrap();
    assert_eq!(
        imported_member(&receiver, &r.manifest.share_id)
            .unwrap()
            .as_deref(),
        Some(text)
    );
    crate::share_import::promote_import(
        &mut receiver,
        &crate::share_import::PromoteImportRequest {
            share_id: r.manifest.share_id.clone(),
            actor: None,
        },
        "2026-10-21T00:00:01Z",
        None,
    )
    .unwrap();
    assert!(receiver.lookup("urn:alignment:left").unwrap().is_none());
    assert!(receiver.lookup("urn:alignment:right").unwrap().is_none());
    assert!(receiver.list_shapes().unwrap().is_empty());
    import_share(&mut receiver, &r, "2026-10-21T00:00:01Z", None).unwrap();
    assert_eq!(
        imported_member(&receiver, &r.manifest.share_id)
            .unwrap()
            .as_deref(),
        Some(text)
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn tar_bytes(p: &SharePayload) -> Vec<u8> {
    let mut tar = tar::Builder::new(Vec::new());
    for (name, text) in &p.files {
        let mut header = tar::Header::new_gnu();
        header.set_size(text.len() as u64);
        header.set_mode(0o600);
        header.set_cksum();
        tar.append_data(&mut header, name, text.as_bytes()).unwrap();
    }
    tar.into_inner().unwrap()
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn directory_and_common_browser_archive_reader_preserve_and_verify_ancillary() {
    let p = payload(Some(EMPTY));
    let dir = tempfile::tempdir().unwrap();
    for (name, text) in &p.files {
        std::fs::write(dir.path().join(name), text).unwrap();
    }
    let local = crate::share_transport::read_local(dir.path().to_str().unwrap()).unwrap();
    assert_eq!(local.alignments_turtle.as_deref(), Some(EMPTY));
    verify_share(&local).unwrap();
    let archive =
        crate::share_transport::read_archive_bytes(&tar_bytes(&p), "private", false).unwrap();
    assert_eq!(archive.alignments_turtle, local.alignments_turtle);
    verify_share(&archive).unwrap();
    std::fs::write(dir.path().join(ALIGNMENTS_FILE), "# changed").unwrap();
    assert!(
        verify_share(&crate::share_transport::read_local(dir.path().to_str().unwrap()).unwrap())
            .is_err()
    );
    std::fs::remove_file(dir.path().join(ALIGNMENTS_FILE)).unwrap();
    assert!(
        verify_share(&crate::share_transport::read_local(dir.path().to_str().unwrap()).unwrap())
            .is_err()
    );
    std::fs::write(dir.path().join(ALIGNMENTS_FILE), [0xff, 0xfe]).unwrap();
    assert!(crate::share_transport::read_local(dir.path().to_str().unwrap()).is_err());
    let mut changed = p.clone();
    changed
        .files
        .insert(ALIGNMENTS_FILE.into(), "# changed".into());
    assert!(
        verify_share(
            &crate::share_transport::read_archive_bytes(&tar_bytes(&changed), "private", false)
                .unwrap()
        )
        .is_err()
    );
    let mut unknown = p;
    unknown.files.insert("unreviewed.ttl".into(), EMPTY.into());
    assert!(
        crate::share_transport::read_archive_bytes(&tar_bytes(&unknown), "private", false).is_err()
    );
}

#[cfg(feature = "remote")]
#[test]
fn http_directory_fetches_only_fixed_manifest_declared_alignment_member() {
    use std::io::{Read, Write};
    let p = payload(Some(EMPTY));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/", listener.local_addr().unwrap());
    let worker = std::thread::spawn(move || {
        let mut paths = Vec::new();
        for _ in 0..4 {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                .unwrap();
            let mut bytes = [0; 4096];
            let n = stream.read(&mut bytes).unwrap();
            let line = std::str::from_utf8(&bytes[..n]).unwrap();
            let name = line
                .split_whitespace()
                .nth(1)
                .unwrap()
                .trim_start_matches('/');
            paths.push(name.to_owned());
            let text = &p.files[name];
            write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                text.len(),
                text
            )
            .unwrap();
        }
        paths
    });
    let r = crate::share_transport::read_url(&url).unwrap();
    assert_eq!(r.alignments_turtle.as_deref(), Some(EMPTY));
    verify_share(&r).unwrap();
    assert_eq!(
        worker.join().unwrap(),
        vec!["manifest.json", "export.nt", "shapes.ttl", ALIGNMENTS_FILE]
    );
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
#[test]
fn native_producer_inputs_refuse_symlink_fifo_and_oversized_files() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let regular = dir.path().join("regular");
    std::fs::write(&regular, EMPTY).unwrap();
    assert_eq!(read_file(&regular).unwrap(), EMPTY);
    let link = dir.path().join("link");
    symlink(&regular, &link).unwrap();
    assert!(read_file(&link).is_err());
    let fifo = dir.path().join("fifo");
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        &fifo,
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    assert!(read_file(&fifo).is_err());
    let huge = dir.path().join("huge");
    std::fs::File::create(&huge)
        .unwrap()
        .set_len(MAX_ALIGNMENT_BYTES as u64 + 1)
        .unwrap();
    assert!(read_file(&huge).is_err());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn delta_carries_complete_sealed_ancillary_and_detects_tampering() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("parent");
    let delta = temp.path().join("delta");
    let store = Store::open_in_memory().unwrap();
    let mut opts = ShareOptions {
        no_shapes: true,
        destination: ShareDestination::Internal,
        ..Default::default()
    };
    crate::share::share(&store, parent.to_str().unwrap(), &opts).unwrap();
    opts.alignments_turtle = Some(EMPTY.into());
    crate::share_delta::write_delta(
        &store,
        parent.to_str().unwrap(),
        delta.to_str().unwrap(),
        &opts,
    )
    .unwrap();
    let r =
        crate::share_delta::materialize(parent.to_str().unwrap(), delta.to_str().unwrap()).unwrap();
    assert_eq!(r.alignments_turtle.as_deref(), Some(EMPTY));
    verify_share(&r).unwrap();
    std::fs::write(delta.join(ALIGNMENTS_FILE), "# corrupt").unwrap();
    let corrupt =
        crate::share_delta::materialize(parent.to_str().unwrap(), delta.to_str().unwrap()).unwrap();
    assert!(verify_share(&corrupt).is_err());
}

#[test]
fn alignment_bytes_cannot_bypass_outward_producer_or_receiver_scrub() {
    let text = "<urn:alignment:x> <urn:description> \"private.svc\" .\n";
    let mut source = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut source);
    let opts = ShareOptions {
        no_shapes: true,
        alignments_turtle: Some(text.into()),
        ..Default::default()
    };
    let error = share_payload(&source, &opts, SHARE_PAYLOAD_MAX_BYTES).unwrap_err();
    assert!(error.to_string().contains(ALIGNMENTS_FILE));
    let p = payload(Some(text));
    let mut r = request(&p);
    r.destination = ShareDestination::Outward;
    let mut receiver = Store::open_in_memory().unwrap();
    crate::share_scrub::seed_test_catalogue(&mut receiver);
    let before = receiver.latest_tx_id().unwrap();
    assert!(import_share(&mut receiver, &r, "2026-10-21T00:00:00Z", None).is_err());
    assert_eq!(receiver.latest_tx_id().unwrap(), before);
    assert!(
        imported_member(&receiver, &r.manifest.share_id)
            .unwrap()
            .is_none()
    );
}
