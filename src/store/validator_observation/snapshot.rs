//! Snapshot extraction never reconstructs selection or fabricates cache provenance.
use super::*;
#[cfg(target_os = "linux")]
use std::collections::{BTreeMap, BTreeSet};
#[cfg(target_os = "linux")]
mod image;

#[cfg(any(target_os = "linux", feature = "owl"))]
pub(super) fn digest(bytes: &[u8]) -> ContentDigest {
    ContentDigest(
        ring::digest::digest(&ring::digest::SHA256, bytes)
            .as_ref()
            .try_into()
            .expect("SHA256 length"),
    )
}
fn named(rows: Vec<(String, String, String)>) -> Vec<NamedContent> {
    rows.into_iter()
        .map(|(name, content, version)| NamedContent {
            name,
            version,
            content: content.into_bytes(),
        })
        .collect()
}
pub(super) fn implementation() -> Option<ValidatorImplementation> {
    #[cfg(not(target_os = "linux"))]
    return None;
    #[cfg(target_os = "linux")]
    {
        use std::io::Read;
        #[cfg(feature = "shacl")]
        let code = [
            implementation as *const () as usize,
            crate::shacl::Validator::validate as *const () as usize,
        ];
        #[cfg(not(feature = "shacl"))]
        let code = [implementation as *const () as usize];
        let mut file = image::open_executable_with_code(&code)?;
        let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
        let mut buffer = [0u8; 65536];
        loop {
            let count = file.read(&mut buffer).ok()?;
            if count == 0 {
                break;
            }
            hash.update(&buffer[..count]);
        }
        let compiled_features: BTreeSet<_> = env!("QUIPU_FEATURES")
            .split_once(';')?
            .1
            .split(',')
            .filter_map(|pair| pair.strip_suffix("=1").map(str::to_owned))
            .collect();
        Some(ValidatorImplementation {
            artifact: ContentDigest(hash.finish().as_ref().try_into().ok()?),
            dependency_lock: digest(include_bytes!("../../../Cargo.lock")),
            compiled_features,
            options: BTreeMap::from([
                ("engine".into(), "shacl-rust/locked".into()),
                ("parser".into(), "Turtle/ReaderMode::Lax".into()),
                ("shape_base".into(), "http://quipu.local/shapes".into()),
                ("data_base".into(), "http://quipu.local/data".into()),
                (
                    "validation_mode".into(),
                    "post-state-owned-components/new-debt-only/v1".into(),
                ),
                (
                    "artifact_kind".into(),
                    "running-executable;verified-code-mappings/v1".into(),
                ),
            ]),
        })
    }
}
impl Store {
    pub(super) fn validator_snapshot(
        &self,
        graph: i64,
        tx: i64,
        conditional: bool,
        implementation: Option<ValidatorImplementation>,
    ) -> Result<(Option<ValidatorManifest>, AdmissionCoordinates)> {
        if self.conn.is_autocommit() {
            return Err(Error::CannotVerify(
                "observation requires reserved writer snapshot".into(),
            ));
        }
        let transaction = self
            .get_transaction(tx)?
            .ok_or_else(|| Error::CannotVerify("missing actual transaction".into()))?;
        let coordinates = AdmissionCoordinates {
            accepted_at: transaction.timestamp,
            transaction: u64::try_from(tx).map_err(|e| Error::CannotVerify(e.to_string()))?,
        };
        let Some(mut implementation) = implementation else {
            return Ok((None, coordinates));
        };
        implementation
            .options
            .insert("store_id".into(), self.store_id()?);
        implementation
            .options
            .insert("graph_id".into(), graph.to_string());
        implementation
            .options
            .insert("transaction".into(), tx.to_string());
        implementation
            .options
            .insert("admission_time".into(), coordinates.accepted_at.clone());
        #[cfg(feature = "shacl")]
        implementation.options.insert(
            "reference_type_graphs".into(),
            format!("{:?}", self.reference_type_graphs(graph)),
        );
        let loaded_shapes = named(self.list_shapes()?);
        let loaded_ontologies = named(self.list_ontologies()?);
        let selected_shapes = Vec::new();
        #[cfg(feature = "owl")]
        let ontology_cache_source = self.owl_cache_source.clone();
        #[cfg(not(feature = "owl"))]
        let ontology_cache_source = None;
        let staged_graph = {
            let facts = self.post_state_facts("SELECT e,a,v,tx,valid_from,valid_to FROM facts WHERE g = ?1 AND op = 1 AND valid_to IS NULL ORDER BY e,a,v,tx", &[graph])?;
            self.cardinality_context(&facts)?.into_bytes()
        };
        let manifest = ValidatorManifest {
            implementation,
            combined_shapes: loaded_shapes.iter().map(|s| String::from_utf8_lossy(&s.content)).collect::<Vec<_>>().join("\n\n").into_bytes(),
            combined_ontologies: loaded_ontologies.iter().map(|s| String::from_utf8_lossy(&s.content)).collect::<Vec<_>>().join("\n").into_bytes(),
            loaded_shapes, loaded_ontologies,
            ontology_cache_source,
            staged_graph,
            incremental: IncrementalContext { conditional, selected_shapes, write_shapes: self.write_shapes.as_ref().map(|s| s.as_bytes().to_vec()), facts: Vec::new(),
                selection_policy: "loaded-if-write-or-conditional;combined-policy-reject+explicit/v1".into(),
                value_node_budget: crate::store::shacl_poststate::CONTEXT_NODE_BUDGET as u64,
                fact_budget: crate::store::shacl_poststate::CONTEXT_FACT_BUDGET as u64,
                fallback_contract: "optional-value-context-exhaustion:truncate-before-values;value-set-only;outer-value-only-exhaustion:skip/v1".into(),
                coverage: ContextCoverage::Unknown },
            flags: ValidatorFlags { shacl_config: self.shacl_config.validate_on_write, owl_validate_on_write: self.owl_config.validate_on_write,
                recording_verdicts: self.recording_verdicts, postcommit_reactive_materialize: self.owl_config.reactive_materialize },
        };
        Ok((Some(manifest), coordinates))
    }
}
