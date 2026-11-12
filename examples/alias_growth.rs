//! Disposable library probe: shared IRIs interned after the reader opens.
use quipu::Store;
use quipu::store::{attach::Attachment, respace::respace_file};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let seed = dir.path().join("seed.db");
    let layer = dir.path().join("layer.db");
    let main = dir.path().join("main.db");
    drop(Store::open(seed.to_str().ok_or("fixture path")?)?);
    respace_file(&seed, &layer, 31)?;
    let reader = Store::open_with_attachments(
        main.to_str().ok_or("fixture path")?,
        &[Attachment::read_only(
            "layer",
            layer.to_str().ok_or("fixture path")?,
        )],
    )?;
    let writer = Store::open(layer.to_str().ok_or("fixture path")?)?;
    println!("fixture=disposable-sqlite reader=opened-before-new-terms");
    let local = reader.intern("urn:demo:late-shared")?;
    let remote = writer.intern("urn:demo:late-shared")?;
    assert_ne!(local, remote);
    println!("same-iri-different-file-ids=true");
    println!(
        "late-shared-canonical={}",
        reader.canonical_id(remote)? == local
    );
    println!(
        "local-id-unchanged={}",
        reader.canonical_id(local)? == local
    );
    let remote = writer.intern("urn:demo:future-local")?;
    println!(
        "remote-only-unchanged={}",
        reader.canonical_id(remote)? == remote
    );
    let local = reader.intern("urn:demo:future-local")?;
    println!(
        "later-local-spelling-canonical={}",
        reader.canonical_id(remote)? == local
    );
    Ok(())
}
