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

/// Checksum-addressed mirror tried after the pinned upstream URL. The
/// upstream release location (github.com/microsoft/onnxruntime) stopped
/// resolving in November 2026. Layout: `<base>/sha256/<pin>/<file name>`, the
/// same contract as CI's `CI_ARTIFACT_MIRROR_URL`. A mirror cannot change what
/// is built: every download is still checked against the catalogue SHA256.
const DEFAULT_MIRROR: &str = "https://github.com/intigraph/ci-mirror/releases/download";

/// Download the pinned archive from upstream, then from the mirrors, and
/// return the first body that fits the byte budget. The caller verifies the
/// SHA256; a source that serves different bytes fails that check loudly.
fn fetch_pinned(url: &str, sha256: &str) -> Vec<u8> {
    println!("cargo:rerun-if-env-changed=QUIPU_ORT_MIRROR");
    let name = url.rsplit('/').next().expect("runtime archive file name");
    let mut sources = vec![url.to_string()];
    if let Ok(base) = env::var("QUIPU_ORT_MIRROR") {
        if !base.is_empty() {
            sources.push(format!(
                "{}/sha256/{sha256}/{name}",
                base.trim_end_matches('/')
            ));
        }
    }
    sources.push(format!("{DEFAULT_MIRROR}/sha256/{sha256}/{name}"));
    let mut failures = Vec::new();
    for source in &sources {
        let response = match ureq::get(source)
            .timeout(std::time::Duration::from_secs(180))
            .call()
        {
            Ok(response) => response,
            Err(error) => {
                failures.push(format!("{source}: {error}"));
                continue;
            }
        };
        let mut archive = Vec::new();
        match response
            .into_reader()
            .take(MAX_ARCHIVE + 1)
            .read_to_end(&mut archive)
        {
            Ok(_) => return archive,
            Err(error) => failures.push(format!("{source}: {error}")),
        }
    }
    panic!(
        "fetch pinned ONNX Runtime over HTTPS failed from every source; set QUIPU_ORT_ARCHIVE to a verified local copy:\n{}",
        failures.join("\n")
    );
}

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
        archive = fetch_pinned(dist.url, dist.archive_sha256);
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
