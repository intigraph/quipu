//! Build-time, checksum-pinned runtime acquisition. Never executed at runtime.
use sha2::{Digest, Sha256};
use std::{
    env, fs,
    io::{Read, Write},
    path::PathBuf,
};
#[path = "onnx_catalogue.rs"]
mod catalogue;

const MAX_ARCHIVE: u64 = 100 * 1024 * 1024;

pub fn bundle() {
    println!("cargo:rerun-if-env-changed=QUIPU_ORT_ARCHIVE");
    println!("cargo:rerun-if-changed=build/onnx_catalogue.rs");
    let target = env::var("TARGET").expect("Cargo TARGET");
    let dist = catalogue::for_target(&target).unwrap_or_else(|| {
        panic!("no pinned ONNX Runtime bundle for {target}; use --no-default-features --features full with a supplied ONNX Runtime")
    });
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    let cache = out.join("onnxruntime-source.archive");
    let mut archive = Vec::new();
    if let Some(path) = env::var_os("QUIPU_ORT_ARCHIVE") {
        println!("cargo:rerun-if-changed={}", PathBuf::from(&path).display());
        fs::File::open(path)
            .expect("read offline runtime archive")
            .take(MAX_ARCHIVE + 1)
            .read_to_end(&mut archive)
            .expect("read archive");
    } else if cache.try_exists().expect("runtime archive cache metadata") {
        fs::File::open(&cache)
            .expect("runtime archive cache")
            .take(MAX_ARCHIVE + 1)
            .read_to_end(&mut archive)
            .expect("read archive cache");
    } else {
        ureq::get(dist.url)
            .timeout(std::time::Duration::from_secs(180))
            .call()
            .expect("fetch pinned ONNX Runtime over HTTPS")
            .into_reader()
            .take(MAX_ARCHIVE + 1)
            .read_to_end(&mut archive)
            .expect("download archive");
    }
    assert!(
        archive.len() as u64 <= MAX_ARCHIVE,
        "runtime archive exceeds byte budget"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&archive)),
        dist.archive_sha256,
        "ONNX Runtime archive checksum mismatch"
    );
    fs::write(&cache, &archive).expect("retain verified build-time archive");
    let prefix = dist
        .member
        .rsplit_once('/')
        .expect("member path")
        .0
        .strip_suffix("/lib")
        .expect("runtime lib directory");
    let wanted = [
        dist.member.to_string(),
        format!("{prefix}/LICENSE"),
        format!("{prefix}/ThirdPartyNotices.txt"),
    ];
    let mut files: [Option<Vec<u8>>; 3] = [None, None, None];
    if dist.url.ends_with(".zip") {
        let mut zip =
            runtime_zip::ZipArchive::new(std::io::Cursor::new(&archive)).expect("runtime ZIP");
        for (index, name) in wanted.iter().enumerate() {
            let mut entry = zip.by_name(name).expect("required runtime ZIP member");
            let mut bytes = Vec::new();
            (&mut entry)
                .take(64 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .expect("read ZIP member");
            files[index] = Some(bytes);
        }
    } else {
        let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(&archive[..]));
        for entry in tar.entries().expect("runtime tar entries") {
            let mut entry = entry.expect("runtime tar entry");
            let name = entry
                .path()
                .expect("runtime member path")
                .to_string_lossy()
                .into_owned();
            if let Some(index) = wanted
                .iter()
                .position(|want| want.trim_start_matches("./") == name.trim_start_matches("./"))
            {
                assert!(
                    entry.header().entry_type().is_file(),
                    "runtime member must be a regular file"
                );
                assert!(files[index].is_none(), "duplicate runtime member");
                let mut bytes = Vec::new();
                (&mut entry)
                    .take(64 * 1024 * 1024 + 1)
                    .read_to_end(&mut bytes)
                    .expect("read tar member");
                files[index] = Some(bytes);
            }
        }
    }
    let [Some(core), Some(license), Some(notices)] = files else {
        panic!("runtime archive is incomplete")
    };
    assert_eq!(
        core.len(),
        dist.core_size,
        "runtime core byte size mismatch"
    );
    assert_eq!(
        format!("{:x}", Sha256::digest(&core)),
        dist.core_sha256,
        "runtime core checksum mismatch"
    );
    let mut compressed = flate2::write::GzEncoder::new(
        fs::File::create(out.join("onnxruntime.gz")).expect("runtime output"),
        flate2::Compression::default(),
    );
    compressed.write_all(&core).expect("compress runtime");
    compressed.finish().expect("finish compressed runtime");
    fs::write(out.join("onnxruntime.LICENSE"), license).expect("write runtime license");
    fs::write(out.join("onnxruntime.NOTICES"), notices).expect("write runtime notices");
    println!("cargo:rustc-env=QUIPU_ORT_CORE_SHA256={}", dist.core_sha256);
    println!("cargo:rustc-env=QUIPU_ORT_CORE_SIZE={}", dist.core_size);
    println!("cargo:rustc-env=QUIPU_ORT_CORE_NAME={}", dist.name);
}
