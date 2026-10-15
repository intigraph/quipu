//! Target-only workaround for rudof 0.3.25's direct-instance class target index.
//!
//! SHACL class targets traverse the data graph's rdfs:subClassOf* relation.
//! Keep the data unchanged: materializing parent rdf:type assertions would also
//! affect closed shapes, hasValue and other constraints on explicit RDF values.
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use rudof_iri::IriS;
use rudof_rdf::rdf_core::term::{Object, Triple as _};
use rudof_rdf::rdf_core::{NeighsRDF, Rdf, SHACLPath};
use rudof_rdf::rdf_impl::OxigraphInMemory;
use shacl_engine::error::ValidationError;
use shacl_engine::ir::{IRComponent, IRSchema, IRShape, ShapeLabelIdx};
use shacl_engine::validator::engine::{Engine, NativeEngine};
use shacl_engine::validator::nodes::{FocusNodes, ValueNodes};
use shacl_engine::validator::processor::ShaclProcessor;
use shacl_engine::validator::report::ValidationOutcome;
use shacl_engine::validator::{RecursionSemantics, ShaclConfig, ShaclValidationMode};

pub(super) struct SubclassValidation(pub OxigraphInMemory);

impl ShaclProcessor<OxigraphInMemory> for SubclassValidation {
    fn store(&self) -> &OxigraphInMemory {
        &self.0
    }

    fn runner(_: &ShaclValidationMode, config: &ShaclConfig) -> Box<dyn Engine<OxigraphInMemory>> {
        Box::new(SubclassEngine {
            inner: Box::new(NativeEngine::new(config.recursion_semantics())),
            children: Arc::new(HashMap::new()),
        })
    }
}

struct SubclassEngine {
    inner: Box<dyn Engine<OxigraphInMemory>>,
    children: Arc<HashMap<Object, Vec<Object>>>,
}

// The native engine retains responsibility for constraints, recursion and caches.
macro_rules! forward_ref {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {$(
        fn $name(&self, $($arg: $ty),*) -> $ret { self.inner.$name($($arg),*) }
    )*};
}
macro_rules! forward_mut {
    ($($name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {$(
        fn $name(&mut self, $($arg: $ty),*) -> $ret { self.inner.$name($($arg),*) }
    )*};
}

impl Engine<OxigraphInMemory> for SubclassEngine {
    fn build_indexes(&mut self, store: &OxigraphInMemory) -> Result<(), ValidationError> {
        self.inner.build_indexes(store)?;
        let mut children: HashMap<Object, Vec<Object>> = HashMap::new();
        // Use the predicate index: do not clone/scan every literal or retain a
        // second index of all instances in a potentially large data graph.
        let predicate =
            oxrdf::NamedNode::new_unchecked("http://www.w3.org/2000/01/rdf-schema#subClassOf");
        for triple in store
            .triples_with_predicate(&predicate)
            .map_err(ValidationError::new_graph_error::<OxigraphInMemory>)?
        {
            let (subject, _, object) = triple.into_components();
            if !object.is_literal() {
                let parent = OxigraphInMemory::term_as_object(&object)?;
                let child =
                    OxigraphInMemory::term_as_object(&OxigraphInMemory::subject_as_term(&subject))?;
                children.entry(parent).or_default().push(child);
            }
        }
        self.children = Arc::new(children);
        Ok(())
    }

    fn fork(&self) -> Box<dyn Engine<OxigraphInMemory>> {
        Box::new(Self {
            inner: self.inner.fork(),
            children: Arc::clone(&self.children),
        })
    }

    fn target_class(
        &self,
        store: &OxigraphInMemory,
        class: &Object,
    ) -> Result<FocusNodes<OxigraphInMemory>, ValidationError> {
        // Preserve the native allocation path for classes with no subclasses.
        // In particular, a graph without hierarchy edges needs no focus union.
        if !self.children.contains_key(class) {
            return self.inner.target_class(store, class);
        }
        let mut pending = vec![class.clone()];
        let mut visited = HashSet::new();
        let mut focus = HashSet::new();
        while let Some(class) = pending.pop() {
            if !visited.insert(class.clone()) {
                continue;
            }
            // The native direct-instance index is already built and shared by forks.
            focus.extend(self.inner.target_class(store, &class)?);
            if let Some(children) = self.children.get(&class) {
                pending.extend(children.iter().cloned());
            }
        }
        Ok(FocusNodes::new(focus))
    }

    forward_ref! {
        target_node(store: &OxigraphInMemory, node: &Object) -> Result<FocusNodes<OxigraphInMemory>, ValidationError>;
        target_subject_of(store: &OxigraphInMemory, predicate: &IriS) -> Result<FocusNodes<OxigraphInMemory>, ValidationError>;
        target_object_of(store: &OxigraphInMemory, predicate: &IriS) -> Result<FocusNodes<OxigraphInMemory>, ValidationError>;
        implicit_target_class(store: &OxigraphInMemory, shape: &Object) -> Result<FocusNodes<OxigraphInMemory>, ValidationError>;
        has_validated(node: &Object, shape_idx: ShapeLabelIdx) -> bool;
        get_cached_outcome(node: &Object, shape_idx: ShapeLabelIdx) -> Option<ValidationOutcome>;
        recursion_semantics() -> RecursionSemantics;
        is_in_chain(node: &Object, shape_idx: ShapeLabelIdx) -> bool;
    }
    forward_mut! {
        evaluate(store: &OxigraphInMemory, shape: &IRShape, component: &IRComponent,
            value_nodes: &ValueNodes<OxigraphInMemory>, source_shape: Option<&IRShape>,
            maybe_path: Option<&SHACLPath>, shapes_graph: &IRSchema) -> Result<ValidationOutcome, ValidationError>;
        record_validation(node: Object, shape_idx: ShapeLabelIdx, outcome: ValidationOutcome) -> ();
        chain_enter(node: Object, shape_idx: ShapeLabelIdx) -> ();
        chain_exit(node: &Object, shape_idx: ShapeLabelIdx) -> ();
    }
}
