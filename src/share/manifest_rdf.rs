//! RDF view of the canonical share manifest.
use super::{ShareDestination, ShareManifest};

pub(crate) fn manifest_turtle(manifest: &ShareManifest) -> String {
    let id = format!("urn:{}", manifest.share_id);
    let literal = |value: &str| serde_json::to_string(value).expect("string JSON cannot fail");
    let parent = manifest
        .parent_share
        .as_ref()
        .map_or_else(String::new, |parent| {
            format!(" ;\n  prov:wasRevisionOf <urn:{parent}>")
        });
    // Emitted only when set, so an outward manifest.ttl is byte-identical to
    // one produced before this field existed. A reader of the RDF view sees the
    // same marker as a reader of the JSON — the exemption must not be visible
    // in only one of the two representations a consumer might read.
    let merge_parents = manifest
        .merge_parents
        .iter()
        .map(|parent| format!(" ;\n  prov:wasDerivedFrom <urn:{parent}>"))
        .collect::<String>();
    // A third distribution only when the share carries queries, for the same
    // byte-identity reason as the destination marker below.
    let queries = manifest.queries_hash.as_ref().map_or_else(String::new, |hash| {
        format!(
            ",\n  [ a dcat:Distribution ; dcat:mediaType \"text/turtle\" ;\n    \
             dcat:downloadURL <payload:queries.ttl> ;\n    \
             spdx:checksum [ a spdx:Checksum ; spdx:algorithm spdx:checksumAlgorithm_sha256 ;\n      \
             spdx:checksumValue {} ] ]",
            literal(hash.strip_prefix("sha256:").unwrap_or(hash))
        )
    });
    let destination = match manifest.destination {
        Some(ShareDestination::Internal) => " ;\n  quipu:destination \"internal\"",
        _ => "",
    };
    format!(
        r#"@prefix dcat: <http://www.w3.org/ns/dcat#> .
@prefix dct: <http://purl.org/dc/terms/> .
@prefix prov: <http://www.w3.org/ns/prov#> .
@prefix spdx: <http://spdx.org/rdf/terms#> .
@prefix xsd: <http://www.w3.org/2001/XMLSchema#> .
@prefix quipu: <https://quipu.dev/ontology/> .

<{id}> a dcat:Dataset, prov:Entity ;
  dct:identifier {} ;
  prov:generatedAtTime {}^^xsd:dateTime ;
  prov:wasAttributedTo [ a prov:SoftwareAgent ;
    dct:title {} ; quipu:version {} ]{parent}{merge_parents}{destination} ;
  dcat:distribution [ a dcat:Distribution ;
    dcat:mediaType "application/n-triples" ;
    dcat:downloadURL <payload:export.nt> ;
    spdx:checksum [ a spdx:Checksum ; spdx:algorithm spdx:checksumAlgorithm_sha256 ;
      spdx:checksumValue {} ] ],
  [ a dcat:Distribution ; dcat:mediaType "text/turtle" ;
    dcat:downloadURL <payload:shapes.ttl> ;
    spdx:checksum [ a spdx:Checksum ; spdx:algorithm spdx:checksumAlgorithm_sha256 ;
      spdx:checksumValue {} ] ]{queries} .
"#,
        literal(&manifest.share_id),
        literal(&manifest.created_at),
        literal(&manifest.producer.name),
        literal(&manifest.producer.version),
        literal(
            manifest
                .graph_hash
                .strip_prefix("sha256:")
                .unwrap_or(&manifest.graph_hash)
        ),
        literal(
            manifest
                .shapes_hash
                .strip_prefix("sha256:")
                .unwrap_or(&manifest.shapes_hash)
        ),
    )
}
