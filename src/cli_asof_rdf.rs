//! Offline as-of frame conversion. Deliberately ignores no global DB choice.

#[cfg(unix)]
use std::path::Path;

pub fn command(args: &[String]) {
    #[cfg(unix)]
    let result = execute(args);
    #[cfg(not(unix))]
    let result: quipu::Result<quipu::asof_rdf::Receipt> = {
        let _ = args;
        Err(quipu::Error::InvalidValue(
            "asof-rdf needs Unix private-file semantics".into(),
        ))
    };
    match result {
        Ok(receipt) => println!(
            "{}",
            serde_json::to_string(&receipt).expect("receipt serializes")
        ),
        Err(error) => {
            eprintln!("asof-rdf: {error}");
            std::process::exit(2);
        }
    }
}

#[cfg(unix)]
fn execute(args: &[String]) -> quipu::Result<quipu::asof_rdf::Receipt> {
    let bad = || {
        quipu::Error::InvalidValue(
            "use asof-rdf --binding FILE --frames FILE --scratch PRIVATE_EMPTY_DIR".into(),
        )
    };
    let mut binding = None;
    let mut frames = None;
    let mut scratch = None;
    for pair in args.get(2..).ok_or_else(bad)?.chunks(2) {
        let [flag, value] = pair else {
            return Err(bad());
        };
        let target = match flag.as_str() {
            "--binding" => &mut binding,
            "--frames" => &mut frames,
            "--scratch" => &mut scratch,
            _ => return Err(bad()),
        };
        if value.is_empty() || value.starts_with("--") || target.replace(value).is_some() {
            return Err(bad());
        }
    }
    quipu::asof_rdf::files::run(
        Path::new(binding.ok_or_else(bad)?),
        Path::new(frames.ok_or_else(bad)?),
        Path::new(scratch.ok_or_else(bad)?),
    )
}
