//! Audience enforcement at the existing share/delta/pack destination seam.

use std::collections::BTreeMap;

use crate::{Error, Result, Store, entity_namespaces, share_scrub::ShareDestination};

/// Decode common RDF, JSON and percent-encoded spellings before examining every
/// emitted artifact, including nested manifest keys/values. Never rewrite output.
fn decoded(text: &str) -> Result<String> {
    let unicode =
        regex::Regex::new(r"\\(?:u([0-9a-fA-F]{4})|U([0-9a-fA-F]{8}))").expect("constant regex");
    let pairs =
        regex::Regex::new(r"\\u([dD][89aAbB][0-9a-fA-F]{2})\\u([dD][c-fC-F][0-9a-fA-F]{2})")
            .expect("constant surrogate regex");

    let mut text = text.to_owned();
    for _ in 0..16 {
        // Nested JSON strings can escape the backslash itself. Normalize only
        // the inspection copy, then combine UTF-16 pairs before scalar escapes.
        let unescaped = text.replace("\\\\", "\\");
        let paired = pairs.replace_all(&unescaped, |caps: &regex::Captures<'_>| {
            let high = u32::from_str_radix(&caps[1], 16).expect("high surrogate");
            let low = u32::from_str_radix(&caps[2], 16).expect("low surrogate");
            char::from_u32(0x10000 + ((high - 0xd800) << 10) + low - 0xdc00)
                .expect("paired Unicode scalar")
                .to_string()
        });
        let next = unicode.replace_all(&paired, |caps: &regex::Captures<'_>| {
            u32::from_str_radix(
                caps.get(1)
                    .or_else(|| caps.get(2))
                    .expect("unicode hex")
                    .as_str(),
                16,
            )
            .ok()
            .and_then(char::from_u32)
            .map_or_else(|| caps[0].to_string(), |c| c.to_string())
        });
        let bytes = next.as_bytes();
        let mut decoded = Vec::with_capacity(bytes.len());
        let mut index = 0;
        while index < bytes.len() {
            if bytes[index] == b'%'
                && index + 2 < bytes.len()
                && bytes[index + 1].is_ascii_hexdigit()
                && bytes[index + 2].is_ascii_hexdigit()
            {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).expect("hex ASCII");
                decoded.push(u8::from_str_radix(hex, 16).expect("hex bytes"));
                index += 3;
            } else {
                decoded.push(bytes[index]);
                index += 1;
            }
        }
        let next = String::from_utf8_lossy(&decoded).replace("\\/", "/");
        if next == text {
            return Ok(text);
        }
        text = next;
    }
    Err(Error::CannotVerify(
        "namespace export escaping exceeds verification bound".into(),
    ))
}

pub(crate) fn enforce(
    store: &Store,
    files: &BTreeMap<String, String>,
    destination: &ShareDestination,
) -> Result<()> {
    let audience = entity_namespaces::destination_audience(store, destination)?;
    let restricted = entity_namespaces::list(store)?
        .into_iter()
        .filter(|ns| ns.audience != "public" && ns.audience != audience)
        .collect::<Vec<_>>();
    if restricted.is_empty() {
        return Ok(());
    }
    for (name, text) in files {
        let text = decoded(text)?;
        for ns in &restricted {
            if text.contains(&decoded(&ns.base_iri)?) {
                // No private IRI, receiver identity or content excerpt in errors.
                return Err(Error::PolicyDenied(format!(
                    "namespace audience refused artifact {name}; destination lacks the namespace audience"
                )));
            }
        }
    }
    Ok(())
}

/// Validate the carried audience against the actual receiver before any staging.
pub(crate) fn carried_namespaces(
    store: &Store,
    destination: &ShareDestination,
) -> Result<Vec<entity_namespaces::Namespace>> {
    let audience = entity_namespaces::destination_audience(store, destination)?;
    let entries = entity_namespaces::list(store)?
        .into_iter()
        .filter(|ns| ns.audience != "public" && ns.audience == audience)
        .collect::<Vec<_>>();
    for entry in &entries {
        entity_namespaces::require_registered(store, entry)?;
    }
    Ok(entries)
}

/// An org transfer is not a general identifier-policy exemption. Exempt only
/// the exact owner-authorized namespace IRIs in the inspection copy, then run
/// the existing catalogue on everything else. Emitted bytes are untouched.
pub(crate) fn scrub_audience(
    store: &Store,
    files: &BTreeMap<String, String>,
    destination: &ShareDestination,
) -> Result<()> {
    let permitted = carried_namespaces(store, destination)?;
    let mut inspection = BTreeMap::new();
    for (name, text) in files {
        let mut text = decoded(text)?;
        for ns in &permitted {
            let pattern = format!(r#"{}[^\s<>"'\\]*"#, regex::escape(&decoded(&ns.base_iri)?));
            text = regex::Regex::new(&pattern)
                .map_err(|_| Error::CannotVerify("namespace inspection pattern".into()))?
                .replace_all(&text, "urn:quipu:permitted-namespace")
                .into_owned();
            if let Some(resolver) = &ns.resolver_url {
                text = text.replace(&decoded(resolver)?, "urn:quipu:permitted-resolver");
            }
        }
        // The old scrub intentionally omits manifest.json; this inspection
        // needs to cover its remaining nested values as well.
        inspection.insert(format!("namespace inspection {name}"), text);
    }
    crate::share_scrub::scrub_outward_payload(store, &inspection, "audience transfer")
}

pub(crate) fn receive(
    store: &Store,
    request: &crate::share_import::ShareImportRequest,
) -> Result<()> {
    let carried = request.manifest.destination.as_ref();
    let declared = &request.destination;
    if let Some(ShareDestination::Audience(binding)) = carried {
        if !matches!(declared, ShareDestination::Audience(local) if local == binding)
            || binding.store_id != store.store_id()?
        {
            return Err(Error::PolicyDenied(
                "namespace audience does not name this receiving store".into(),
            ));
        }
        entity_namespaces::destination_audience(store, declared)?;
    }
    let local = entity_namespaces::list(store)?;
    for entry in &request.manifest.namespaces {
        entity_namespaces::require_registered(store, entry)?;
    }
    if request
        .manifest
        .namespaces
        .iter()
        .any(|ns| !local.contains(ns))
    {
        return Err(Error::PolicyDenied(
            "imported namespace declaration lacks matching owner policy".into(),
        ));
    }
    let mut files = BTreeMap::from([
        (
            "manifest.json".into(),
            serde_json::to_string(&request.manifest)
                .map_err(|e| Error::Serialization(e.to_string()))?,
        ),
        ("export.nt".into(), request.export_ntriples.clone()),
        ("shapes.ttl".into(), request.shapes_turtle.clone()),
    ]);
    if let Some(queries) = &request.queries_turtle {
        files.insert("queries.ttl".into(), queries.clone());
    }
    enforce(store, &files, declared)?;
    if matches!(declared, ShareDestination::Audience(_)) {
        scrub_audience(store, &files, declared)?;
    }
    Ok(())
}

pub(crate) fn receive_pack(
    store: &Store,
    manifest: &crate::pack::Manifest,
    declared: &ShareDestination,
    canonical: &str,
) -> Result<()> {
    if let Some(ShareDestination::Audience(binding)) = &manifest.destination {
        if !matches!(declared, ShareDestination::Audience(local) if local == binding)
            || binding.store_id != store.store_id()?
            || !entity_namespaces::list(store)?
                .iter()
                .any(|ns| ns.audience == binding.audience)
        {
            return Err(Error::PolicyDenied(
                "pack audience lacks receiver identity/policy proof".into(),
            ));
        }
        entity_namespaces::destination_audience(store, declared)?;
    }
    let files = BTreeMap::from([
        ("pack content".into(), canonical.into()),
        (
            "pack manifest".into(),
            serde_json::to_string(manifest).map_err(|e| Error::Serialization(e.to_string()))?,
        ),
    ]);
    enforce(store, &files, declared)?;
    if matches!(declared, ShareDestination::Audience(_)) {
        scrub_audience(store, &files, declared)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "namespace_publication_tests.rs"]
mod tests;
