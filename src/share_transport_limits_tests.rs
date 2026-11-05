use super::*;

fn compressed_members(sizes: &[usize]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut archive = tar::Builder::new(encoder);
    for (name, size) in ["export.nt", "export.ttl"].into_iter().zip(sizes) {
        let mut header = tar::Header::new_gnu();
        header.set_size(*size as u64);
        header.set_mode(0o644);
        header.set_cksum();
        archive
            .append_data(&mut header, name, std::io::repeat(b' ').take(*size as u64))
            .unwrap();
    }
    archive.into_inner().unwrap().finish().unwrap()
}

#[test]
fn expanded_limit_accepts_measured_release_and_exact_boundary_but_refuses_one_more_byte() {
    // Same total as the checksum-verified v0.17 release, not a tiny fixture that
    // would also pass under the broken 64 MiB bound. The transport sees both views.
    for total in [69_411_571, 96 * 1024 * 1024] {
        let first = total / 2;
        let compressed = compressed_members(&[first, total - first]);
        assert!(compressed.len() < MAX_SHARE_DOWNLOAD_BYTES);
        let files = archive_files(
            flate2::read::GzDecoder::new(Cursor::new(compressed)),
            "measured release size / exact boundary",
        )
        .unwrap();
        assert_eq!(files.values().map(String::len).sum::<usize>(), total);
    }
    // Split across files: accounting must be cumulative, not per member.
    let compressed = compressed_members(&[48 * 1024 * 1024, 48 * 1024 * 1024 + 1]);
    let error = archive_files(
        flate2::read::GzDecoder::new(Cursor::new(compressed)),
        "expanded bomb",
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("exceeds 100663296 expanded bytes")
    );
}

#[test]
fn compressed_response_bound_remains_sixteen_mib() {
    let error =
        read_archive_bytes(&vec![0; 16 * 1024 * 1024 + 1], "compressed bomb", true).unwrap_err();
    assert!(error.to_string().contains("exceeds 16777216 bytes"));
}
