use super::*;

fn fixture(root: &Path) -> Receipt {
    fs::create_dir_all(root.join("lib")).unwrap();
    fs::create_dir_all(root.join("licenses")).unwrap();
    fs::write(
        root.join("licenses/LICENSE"),
        b"fixture redistribution notice",
    )
    .unwrap();
    let mut files = BTreeMap::new();
    for name in [
        CORE,
        "libonnxruntime_providers_cuda.so",
        "libonnxruntime_providers_shared.so",
    ] {
        fs::write(root.join("lib").join(name), name.as_bytes()).unwrap();
        files.insert(
            name.to_string(),
            hex::encode(ring::digest::digest(&ring::digest::SHA256, name.as_bytes())),
        );
    }
    fs::write(
        root.join("receipt.json"),
        serde_json::to_vec(&serde_json::json!({"version":VERSION,"files":files})).unwrap(),
    )
    .unwrap();
    Receipt {
        version: VERSION.into(),
        files,
    }
}

#[test]
fn matched_bundle_stages_then_refuses_changed_library_and_forged_receipt() {
    let temp = tempfile::tempdir().unwrap();
    let source = temp.path().join("source");
    let expected = fixture(&source);
    let cache = temp.path().join("cache");
    let core = provision(&source, &cache, &expected).unwrap();
    assert!(core.is_file());
    assert_eq!(
        fs::read(cache.join(VERSION).join("licenses/LICENSE")).unwrap(),
        b"fixture redistribution notice"
    );
    fs::write(
        source.join("lib/libonnxruntime_providers_cuda.so"),
        b"wrong-version",
    )
    .unwrap();
    assert!(verify(&source, &expected).is_err());
    let mut forged = expected.files.clone();
    forged.insert(
        "libonnxruntime_providers_cuda.so".into(),
        hex::encode(ring::digest::digest(
            &ring::digest::SHA256,
            b"wrong-version",
        )),
    );
    fs::write(
        source.join("receipt.json"),
        serde_json::to_vec(&serde_json::json!({"version":VERSION,"files":forged})).unwrap(),
    )
    .unwrap();
    assert!(
        verify(&source, &expected).is_err(),
        "receipt cannot bless new bytes"
    );
    assert!(verify(&cache.join(VERSION), &expected).is_ok());
}

#[test]
fn missing_provider_and_symlink_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let expected = fixture(temp.path());
    let shared = temp.path().join("lib/libonnxruntime_providers_shared.so");
    fs::remove_file(&shared).unwrap();
    assert!(verify(temp.path(), &expected).is_err());
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(temp.path().join("lib").join(CORE), shared).unwrap();
        assert!(verify(temp.path(), &expected).is_err());
    }
}

fn archive_bytes(files: &BTreeMap<String, String>, extra: Option<&str>) -> Vec<u8> {
    let mut zip = runtime_zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for name in files.keys() {
        zip.start_file(
            format!("wheel/lib/{name}"),
            runtime_zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        zip.write_all(name.as_bytes()).unwrap();
    }
    if let Some(name) = extra {
        zip.start_file(name, runtime_zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(b"bad").unwrap();
    }
    zip.finish().unwrap().into_inner()
}

#[test]
fn empty_cache_acquisition_is_atomic_and_offline_errors_remain_absent() {
    let temp = tempfile::tempdir().unwrap();
    let expected = fixture(&temp.path().join("source"));
    let bytes = archive_bytes(&expected.files, None);
    let artifacts = [Artifact {
        name: "fixture".into(),
        url: "https://files.pythonhosted.org/fixture".into(),
        sha256: hex::encode(ring::digest::digest(&ring::digest::SHA256, &bytes)),
    }];
    let cache = temp.path().join("cache");
    let failed = acquire_from(&cache, &expected, &artifacts, |_, _| {
        Err(error("offline fixture"))
    });
    assert!(failed.is_err());
    assert!(!cache.join(VERSION).exists());
    let core = acquire_from(&cache, &expected, &artifacts, |_, out| {
        out.write_all(&bytes).map_err(error)
    })
    .unwrap();
    assert!(core.exists());
    acquire_from(&cache, &expected, &artifacts, |_, _| {
        panic!("valid cache must not fetch")
    })
    .unwrap();
    assert!(!cache.read_dir().unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".gpu-download-")
    }));
}

#[test]
fn changed_archive_unsafe_member_and_duplicate_library_never_publish() {
    let temp = tempfile::tempdir().unwrap();
    let expected = fixture(&temp.path().join("source"));
    let good = archive_bytes(&expected.files, None);
    let artifact = Artifact {
        name: "fixture".into(),
        url: "https://files.pythonhosted.org/fixture".into(),
        sha256: hex::encode(ring::digest::digest(&ring::digest::SHA256, &good)),
    };
    let cache = temp.path().join("cache");
    assert!(acquire_from(&cache, &expected, &[artifact], |_, out| out
        .write_all(b"not the pinned ZIP")
        .map_err(error))
    .is_err());
    assert!(!cache.join(VERSION).exists());
    for extra in ["../escape", "other/libonnxruntime_providers_shared.so"] {
        let bytes = archive_bytes(&expected.files, Some(extra));
        let artifact = Artifact {
            name: "fixture".into(),
            url: "https://files.pythonhosted.org/fixture".into(),
            sha256: hex::encode(ring::digest::digest(&ring::digest::SHA256, &bytes)),
        };
        assert!(acquire_from(&cache, &expected, &[artifact], |_, out| out
            .write_all(&bytes)
            .map_err(error))
        .is_err());
        assert!(!cache.join(VERSION).exists());
    }
}
