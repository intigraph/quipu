//! Store context for write-time SHACL (aegis-fp17f).
//!
//! ## The defect this exists to remove
//!
//! SHACL here validated the SUBMITTED payload in isolation. That is defensible
//! for constraints about what a write ASSERTS, and wrong for constraints about
//! what a write REFERENCES: `sh:class` on an object property asks "is this value
//! a Foo?", and the answer lives in the graph, not in the request body. So a
//! correct write was refused whenever the referenced node's `rdf:type` triple
//! happened not to travel with it.
//!
//! The consequence is worse than strictness. It made the verdict a function of
//! how a caller PARTITIONED its triples: the same facts, submitted whole,
//! conformed; submitted split across two writes, the second was refused.
//! Measured at scale on hank's chunked promotion (aegis-sd5fj) — 2315 of 7638
//! symbols refused at chunk 2 of 71, every one of them a correct fact about a
//! module that was already typed in the store. A validator whose answer depends
//! on submission order is not reporting conformance, it is reporting an
//! accident of framing.
//!
//! ## What this does instead: REPAIR, never re-judge
//!
//! Validation runs against the payload ALONE first. That verdict — today's
//! verdict, unchanged — is the ceiling. Only if it refuses does a second pass
//! run with the store's type triples added, and the second pass may only
//! REMOVE violations from the first. The reported result is always a SUBSET of
//! what the payload alone produced.
//!
//! So "strictly more permissive" is not an argument about the code, it is the
//! shape of the code: there is no path on which a violation the old behaviour
//! did not report can be returned. A conforming payload takes the fast path and
//! costs exactly one validation, as before; only a payload that was already
//! being refused pays for the second pass.
//!
//! ## Why not the obvious implementation
//!
//! The first version of this module added the context and then dropped
//! violations whose focus node was not a payload SUBJECT — reasoning that a
//! write is answerable only for what it describes. That reasoning is wrong, and
//! the at-scale measurement caught it where the argument did not:
//! `partition_soak` found chunk 4 of 5 newly REFUSED on a real 65 MB
//! projection. A module can be a payload subject via an `imports` edge while
//! the payload does not describe it at all; adding its type from the store then
//! made it a target of `CodeModuleShape`, whose `sh:minCount filePath` had
//! nothing to satisfy it — a write refused for a shape violation on a node it
//! merely mentioned.
//!
//! That is precisely the "a tightening refuses writes that currently succeed"
//! hazard, arrived at while trying to avoid it. The subset property above is
//! the fix, and it is structural rather than reasoned, which is the difference
//! that matters: the earlier version was also carefully reasoned.

use std::collections::{BTreeMap, BTreeSet};

use oxrdf::{NamedOrBlankNode, Term as OxTerm};
use oxrdfio::{RdfFormat, RdfParser};

use crate::error::{Error, Result};
use crate::store::Store;
use crate::types::Value;

/// `rdf:type`, spelled in full because that is how it is interned.
const RDF_TYPE: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#type";

/// What a payload asserts, split into the parts write-time validation needs.
#[derive(Debug, Default)]
pub struct PayloadScope {
    /// Subjects the payload DESCRIBES — the only nodes this write answers for.
    pub subjects: BTreeSet<String>,
    /// IRIs the payload references in object position without typing them here.
    /// These are the nodes whose type may need to come from the store.
    pub untyped_references: BTreeSet<String>,
    /// Named objects per predicate. A `sh:targetObjectsOf` shape makes these
    /// FOCUS nodes even though the payload does not describe them.
    pub objects: BTreeMap<String, BTreeSet<String>>,
}

/// Read a payload's subjects and its untyped outbound references.
///
/// Parses rather than scans: a chunk is Turtle, and a string search for
/// `a bobbin:Foo` would miss prefixed forms, full-IRI `rdf:type`, and predicate
/// lists, each of which changes the answer in the unsafe direction (a reference
/// wrongly believed to be typed here means no context is fetched and the write
/// is refused exactly as before).
pub fn scope_of(payload_turtle: &str) -> Result<PayloadScope> {
    let mut subjects = BTreeSet::new();
    let mut typed_here = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    let mut objects: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

    let parser = RdfParser::from_format(RdfFormat::Turtle);
    for quad in parser.for_reader(payload_turtle.as_bytes()) {
        let quad = quad.map_err(|e| Error::InvalidValue(format!("RDF parse error: {e}")))?;
        if let NamedOrBlankNode::NamedNode(s) = &quad.subject {
            subjects.insert(s.as_str().to_string());
            if quad.predicate.as_str() == RDF_TYPE {
                typed_here.insert(s.as_str().to_string());
            }
        }
        if let OxTerm::NamedNode(o) = &quad.object {
            // Every predicate, rdf:type included: `sh:targetObjectsOf rdf:type`
            // makes the CLASS a focus node just like any other predicate does.
            objects
                .entry(quad.predicate.as_str().to_string())
                .or_default()
                .insert(o.as_str().to_string());
            // Type repair stays scoped to non-type references: a class named by
            // rdf:type is not a value whose `sh:class` needs the store's type.
            if quad.predicate.as_str() != RDF_TYPE {
                referenced.insert(o.as_str().to_string());
            }
        }
    }

    // A node the payload types needs nothing from the store; a node the payload
    // merely mentions might.
    let untyped_references = referenced.difference(&typed_here).cloned().collect();
    Ok(PayloadScope {
        subjects,
        untyped_references,
        objects,
    })
}

/// The `rdf:type` triples the STORE holds for `iris`, as N-Triples.
///
/// Types ONLY. Pulling each referenced node's full description would be the
/// obvious generalisation and is deliberately not done: it turns one write into
/// an unbounded read, and every extra triple is another chance to make a node
/// the payload does not own into a validation target. `sh:class` needs the type
/// and nothing else.
///
/// Unknown IRIs are skipped silently — a reference to something the store has
/// never seen is not an error here, it is simply a constraint that will fail on
/// its own merits.
pub fn store_type_context(store: &Store, iris: &BTreeSet<String>) -> Result<String> {
    store_type_context_in_graph(store, iris, crate::schema::ROOT_GRAPH)
}

/// [`store_type_context`], scoped to the graph a write is landing in (quipu-080).
///
/// The context is the UNION of the target graph's types and ROOT's. That is the
/// semantics `docs/design/named-graphs.md` already sanctions for the shapes
/// half: ontology lives in ROOT and applies to every destination graph, so a
/// node ROOT types (an `aegis:` class, a shared entity) repairs a plane-routed
/// write exactly as it repairs a ROOT write. The graph's own half is the fix
/// for the aegis-fp17f/aegis-sd5fj defect class re-surfacing on `/knot`'s
/// `graph` param: a chunked write into a named committed graph whose earlier
/// chunks typed nodes IN THAT GRAPH must see those types too.
///
/// What is deliberately NOT unioned: any third graph. Planes are trust
/// boundaries — camayoc's ingress discipline quarantines model-inferred facts
/// in low-trust planes precisely so they cannot masquerade — and a quarantined
/// graph's `rdf:type` claims repairing a write into another plane would be
/// exactly that masquerade. Deduplicated because ROOT and the target graph may
/// both hold the same type triple, and `g = ROOT` reduces to the old behaviour.
pub fn store_type_context_in_graph(
    store: &Store,
    iris: &BTreeSet<String>,
    g: i64,
) -> Result<String> {
    let Some(type_attr) = store.lookup(RDF_TYPE)? else {
        // No rdf:type interned at all — an empty store. No context to add.
        return Ok(String::new());
    };
    let mut lines = BTreeSet::new();
    let mut graphs = vec![crate::schema::ROOT_GRAPH];
    if g != crate::schema::ROOT_GRAPH {
        graphs.push(g);
    }
    for iri in iris {
        let Some(entity) = store.lookup(iri)? else {
            continue;
        };
        for &scope in &graphs {
            for fact in store.entity_facts_in_graph(entity, scope)? {
                if fact.attribute != type_attr {
                    continue;
                }
                if let Value::Ref(type_id) = fact.value {
                    let type_iri = store.resolve(type_id)?;
                    lines.insert(format!("<{iri}> <{RDF_TYPE}> <{type_iri}> .\n"));
                }
            }
        }
    }
    Ok(lines.into_iter().collect())
}

/// The predicates named by `sh:targetObjectsOf` in `shapes_turtle`.
pub fn object_target_predicates(shapes_turtle: &str) -> Result<BTreeSet<String>> {
    const SH_TARGET_OBJECTS_OF: &str = "http://www.w3.org/ns/shacl#targetObjectsOf";
    let parser = oxttl::TurtleParser::new()
        .with_base_iri("http://example.org/")
        .map_err(|e| Error::InvalidValue(format!("shapes base IRI: {e}")))?;
    let mut predicates = BTreeSet::new();
    for triple in parser.for_reader(shapes_turtle.as_bytes()) {
        let triple = triple.map_err(|e| Error::InvalidValue(format!("shapes parse error: {e}")))?;
        if let OxTerm::NamedNode(p) = &triple.object
            && triple.predicate.as_str() == SH_TARGET_OBJECTS_OF
        {
            predicates.insert(p.as_str().to_string());
        }
    }
    Ok(predicates)
}

/// SHACL paths that some property shape REQUIRES (`sh:minCount >= 1`).
///
/// Deliberately not per-`sh:targetClass`: the answer is only used to decide
/// which of a payload subject's EXISTING facts are worth fetching, and fetching
/// a fact that turns out to be irrelevant costs one row and cannot change a
/// verdict (see [`repaired`]). Resolving path-to-class properly would mean
/// implementing target selection a second time, beside the validator that
/// already does it — two implementations that must agree, and only one of them
/// exercised by the tests.
///
/// The set is small in practice: the shape files hold a label floor plus a
/// handful of emitter-proven scalars.
pub fn required_paths(shapes_turtle: &str) -> Result<BTreeSet<String>> {
    use oxttl::TurtleParser;
    use std::collections::BTreeMap;

    const SH_PATH: &str = "http://www.w3.org/ns/shacl#path";
    const SH_MIN_COUNT: &str = "http://www.w3.org/ns/shacl#minCount";

    let parser = TurtleParser::new()
        .with_base_iri("http://example.org/")
        .map_err(|e| Error::InvalidValue(format!("shapes base IRI: {e}")))?;

    // A property shape is usually a BLANK node (`sh:property [ ... ]`), so the
    // path and the count arrive as two triples about the same subject and have
    // to be joined rather than read off one.
    let mut path_of: BTreeMap<String, String> = BTreeMap::new();
    let mut required: BTreeSet<String> = BTreeSet::new();

    for triple in parser.for_reader(shapes_turtle.as_bytes()) {
        let triple = triple.map_err(|e| Error::InvalidValue(format!("shapes parse error: {e}")))?;
        let subject = triple.subject.to_string();
        match triple.predicate.as_str() {
            SH_PATH => {
                if let OxTerm::NamedNode(path) = &triple.object {
                    path_of.insert(subject, path.as_str().to_string());
                }
            }
            SH_MIN_COUNT => {
                if let OxTerm::Literal(lit) = &triple.object
                    && lit.value().parse::<i64>().is_ok_and(|n| n >= 1)
                {
                    required.insert(subject);
                }
            }
            _ => {}
        }
    }

    Ok(required
        .into_iter()
        .filter_map(|s| path_of.get(&s).cloned())
        .collect())
}

/// The store's values for `paths` on the payload's own SUBJECTS, as N-Triples.
///
/// # Why subjects, when [`store_type_context_in_graph`] is about references
///
/// That function fixes `sh:class` on a node the payload MENTIONS. This one
/// fixes a different defect with the opposite shape (aegis-dixug): adding a
/// governed type to an EXISTING, fully conformant node is refused unless the
/// payload also restates every property that node's shape requires — because
/// `sh:targetClass` matching sees the type in the payload, while every OTHER
/// constraint on that shape is evaluated against the payload alone.
///
/// ```text
/// # store already holds: <guard> rdfs:label "guard"
/// POST /knot  "<guard> a aegis:OperationalRule ."
///   -> conforms:false, MinCount(1) not satisfied, path=rdfs:label
/// ```
///
/// The message names a label that is RIGHT THERE, so it sends the caller
/// looking for a missing fact, finding one, and concluding the store is
/// inconsistent. That is not a corner case: it is THE SHAPE OF EVERY
/// INCREMENTAL WRITE — adding a type, an edge, one property. The narrower and
/// more careful the write, the likelier it is refused.
///
/// # Why this is bounded, unlike the generalisation the module warns off
///
/// [`store_type_context_in_graph`] declines to pull each referenced node's full
/// description because that turns one write into an unbounded read. This is not
/// that: it is restricted to paths some shape REQUIRES, so it fetches the
/// handful of scalars a floor is made of, never a node's edge set. A payload
/// subject is also a node the write ANSWERS for, which a mere reference is not.
///
/// Safety is structural either way — [`repaired`] subtracts, so no context can
/// introduce a violation into the reported result. This can only turn a refusal
/// into a pass, never the reverse.
pub fn store_property_context_in_graph(
    store: &Store,
    subjects: &BTreeSet<String>,
    paths: &BTreeSet<String>,
    g: i64,
) -> Result<String> {
    if subjects.is_empty() || paths.is_empty() {
        return Ok(String::new());
    }
    // Resolve the required paths to term ids ONCE. A path the store has never
    // interned cannot be on any fact, so it drops out here rather than being
    // compared per fact.
    let mut wanted = BTreeSet::new();
    for path in paths {
        if let Some(id) = store.lookup(path)? {
            wanted.insert(id);
        }
    }
    if wanted.is_empty() {
        return Ok(String::new());
    }

    let mut graphs = vec![crate::schema::ROOT_GRAPH];
    if g != crate::schema::ROOT_GRAPH {
        graphs.push(g);
    }

    let mut lines = BTreeSet::new();
    for iri in subjects {
        let Some(entity) = store.lookup(iri)? else {
            continue;
        };
        for &scope in &graphs {
            for fact in store.entity_facts_in_graph(entity, scope)? {
                if !wanted.contains(&fact.attribute) {
                    continue;
                }
                let attr = store.resolve(fact.attribute)?;
                // A value this store holds but cannot render as an RDF term
                // (raw bytes) is skipped, never fatal: this is a repair pass
                // beside a write that has already been judged once.
                let Ok(term) = crate::rdf::value_to_term(store, &fact.value) else {
                    continue;
                };
                lines.insert(format!("<{iri}> <{attr}> {term} .\n"));
            }
        }
    }
    Ok(lines.into_iter().collect())
}

/// Validate `turtle` against `shapes`, using the store to REPAIR violations
/// caused by context the payload does not carry.
///
/// This is the write-time entry point. `shacl::validate_shapes` remains the
/// context-free form and is still correct for callers that genuinely have no
/// store — `/validate` with caller-supplied shapes, and the shape-authoring
/// tests.
#[cfg(feature = "shacl")]
pub fn validate_with_store_context(
    store: &Store,
    shapes_turtle: &str,
    data_turtle: &str,
) -> Result<crate::shacl::ValidationFeedback> {
    validate_with_store_context_in_graph(
        store,
        shapes_turtle,
        data_turtle,
        crate::schema::ROOT_GRAPH,
    )
}

/// [`validate_with_store_context`] for a write landing in graph `g` (quipu-080):
/// the repair context is the target graph's type triples unioned with ROOT's —
/// see [`store_type_context_in_graph`] for why that union and no wider. `/knot`
/// threads its resolved destination graph here; `g = ROOT` is byte-identical to
/// the two-argument form.
#[cfg(feature = "shacl")]
pub fn validate_with_store_context_in_graph(
    store: &Store,
    shapes_turtle: &str,
    data_turtle: &str,
    g: i64,
) -> Result<crate::shacl::ValidationFeedback> {
    // Pass 1: the payload alone. This is the OLD behaviour and the ceiling on
    // what may be reported.
    let baseline = crate::shacl::validate_shapes(shapes_turtle, data_turtle)?;
    if baseline.conforms {
        // Fast path: nothing to repair, and no second validation is paid for.
        return Ok(baseline);
    }

    let scope = scope_of(data_turtle)?;
    // Two repairs, deliberately additive and deliberately different in kind:
    //   - TYPES for nodes the payload merely REFERENCES  (aegis-fp17f/sd5fj):
    //     fixes `sh:class` on a value whose type lives in the store.
    //   - REQUIRED PROPERTIES on the payload's own SUBJECTS (aegis-dixug):
    //     fixes an incremental write being refused for a floor the store
    //     already satisfies. Objects of a `sh:targetObjectsOf` predicate are
    //     focus nodes too (aegis-g51c C1): an edge-only write to a stored,
    //     valid node must see that node's required properties.
    // Both are bounded, and neither can add a violation — `repaired` subtracts.
    let mut context = store_type_context_in_graph(store, &scope.untyped_references, g)?;
    let mut focuses = scope.subjects.clone();
    for predicate in object_target_predicates(shapes_turtle)? {
        focuses.extend(scope.objects.get(&predicate).into_iter().flatten().cloned());
    }
    context.push_str(&store_property_context_in_graph(
        store,
        &focuses,
        &required_paths(shapes_turtle)?,
        g,
    )?);
    if context.is_empty() {
        // Nothing to repair with; a node in pre-existing debt lands here, since
        // it lacks the very values the repair would add.
        return admit_preexisting_debt(store, shapes_turtle, baseline, g);
    }

    // Pass 2: the same payload with the store's type triples. Turtle and
    // N-Triples concatenate cleanly, and the context is written in full-IRI
    // form precisely so it cannot depend on the payload's prefixes.
    let augmented =
        crate::shacl::validate_shapes(shapes_turtle, &format!("{data_turtle}\n{context}"))?;
    let feedback = repaired(baseline, &augmented);
    if feedback.conforms {
        return Ok(feedback);
    }
    admit_preexisting_debt(store, shapes_turtle, feedback, g)
}

/// Prefix on the message of a violation admitted as pre-existing debt.
pub const DEBT_MESSAGE_PREFIX: &str = "pre-existing debt (admitted as Warning): ";

/// Demote to Warning each remaining violation the store ALREADY had (aegis-g51c C12).
///
/// # The defect
///
/// A writer reusing an existing node by name and type (`<s> a aegis:Session`,
/// the graph-extract reuse form) is judged by the stored node's required
/// fields, through the repair above. When the STORED node was written before
/// its shape existed and lacks a required field, the write is refused for a
/// violation it did not introduce and cannot fix without inventing provenance.
/// Measured in production: a session-capture `/episode` refused for an old
/// codex Session's missing `actor`/`sourceKind` (tx 722359), and 562 more such
/// nodes in ROOT.
///
/// # What is admitted, exactly
///
/// The PRE-STATE of each remaining focus node, meaning its whole stored
/// description plus the types of the nodes it references, is validated on its
/// own. Only focus-local components are admitted ([`is_focus_local`]): for those
/// that context is the complete pre-state, so a pre-state violation is real
/// debt. Nested and logical components (`sh:node`, `sh:qualifiedValueShape`,
/// `sh:or` and kin) depend on other nodes' descriptions; a bounded context could
/// fabricate their pre-state violation, so they are never admitted. A payload violation is admitted only
/// when that pre-state run reports the SAME issue: the same focus, component,
/// path, value, message and, when the shape is named, source shape. Everything
/// else is still refused:
///
/// - a NEW node has no stored types, so its pre-state targets no shape;
/// - a new TYPE on an existing node is not in the pre-state, so its shape's
///   violations have no pre-state twin;
/// - a new VALUE (`sh:class`, `sh:in`, datatype) differs in the key;
/// - a different path on a debt node differs in the key.
///
/// Admitted issues stay visible: each is returned as a Warning with
/// [`DEBT_MESSAGE_PREFIX`] and queued as a `shacl.debt_admitted` event, so the
/// debt can be counted and burnt down rather than becoming permanent.
#[cfg(feature = "shacl")]
fn admit_preexisting_debt(
    store: &Store,
    shapes_turtle: &str,
    mut feedback: crate::shacl::ValidationFeedback,
    g: i64,
) -> Result<crate::shacl::ValidationFeedback> {
    let focuses: BTreeSet<String> = feedback
        .results
        .iter()
        .filter(|r| is_violation(r))
        .map(|r| r.focus_node.clone())
        .collect();
    // Types first: a focus the store does not type cannot carry debt, and an
    // empty type context means every remaining violation is new.
    if store_type_context_in_graph(store, &focuses, g)?.is_empty() {
        return Ok(feedback);
    }
    let pre_state = store_description_in_graph(store, &focuses, g)?;
    let before = crate::shacl::validate_shapes(shapes_turtle, &pre_state)?;
    let debt: BTreeSet<String> = before
        .results
        .iter()
        .filter(|r| is_violation(r) && is_focus_local(r))
        .map(debt_key)
        .collect();
    if debt.is_empty() {
        return Ok(feedback);
    }

    let graph = store.graph_iri_of(g);
    for issue in &mut feedback.results {
        if !is_violation(issue) || !is_focus_local(issue) || !debt.contains(&debt_key(issue)) {
            continue;
        }
        store.queue_write_event(crate::store::PendingWriteEvent {
            event_type: "shacl.debt_admitted".to_string(),
            subject: Some(issue.focus_node.clone()),
            payload: serde_json::json!({
                "shape": issue.source_shape,
                "message": issue.message,
                "component": issue.component,
                "path": issue.path,
                "graph": graph,
            }),
        });
        issue.severity = "Warning".to_string();
        issue.message = Some(format!(
            "{DEBT_MESSAGE_PREFIX}{}",
            issue.message.as_deref().unwrap_or("")
        ));
    }
    feedback.violations = feedback.results.iter().filter(|r| is_violation(r)).count();
    feedback.warnings = feedback
        .results
        .iter()
        .filter(|r| r.severity.to_ascii_lowercase().contains("warning"))
        .count();
    feedback.conforms = feedback.violations == 0;
    Ok(feedback)
}

/// Violations a RETRACTION introduces on the nodes it edits (aegis-g51c C12).
///
/// Write-time SHACL judges what a write asserts, so `/update` could delete a
/// required field and leave a conforming node violating, unseen. Debt admission
/// makes that gap matter: a node made non-conforming by a delete would look like
/// pre-existing debt to every later write. So a delete is judged here against
/// the same pre-state [`admit_preexisting_debt`] uses.
///
/// `removed_nt` and `asserted_nt` are the N-Triples the update deletes from and
/// adds to graph `g`. Only subjects that lose a value on a REQUIRED path are
/// judged; a delete elsewhere cannot fail a `minCount`, and the post-state is
/// built from the same bounded context (stored types and required-path
/// values), so other components are not judged here. A violation in that
/// post-state is returned unless the pre-state already had it, keyed by
/// [`debt_key`]. An empty result means the deletes introduce nothing.
#[cfg(feature = "shacl")]
pub fn retraction_violations_in_graph(
    store: &Store,
    shapes_turtle: &str,
    removed_nt: &str,
    asserted_nt: &str,
    g: i64,
) -> Result<crate::shacl::ValidationFeedback> {
    let required = required_paths(shapes_turtle)?;
    let removed = parse_triples(removed_nt)?;
    let subjects: BTreeSet<String> = removed
        .iter()
        .filter(|t| required.contains(t.predicate.as_str()))
        .filter_map(|t| match &t.subject {
            NamedOrBlankNode::NamedNode(s) => Some(s.as_str().to_string()),
            NamedOrBlankNode::BlankNode(_) => None,
        })
        .collect();
    let mut feedback = crate::shacl::ValidationFeedback {
        conforms: true,
        violations: 0,
        warnings: 0,
        results: Vec::new(),
        resolution_candidates: Default::default(),
    };
    if subjects.is_empty() {
        return Ok(feedback);
    }

    let pre_nt = format!(
        "{}{}",
        store_type_context_in_graph(store, &subjects, g)?,
        store_property_context_in_graph(store, &subjects, &required, g)?
    );
    let mut post: std::collections::HashSet<oxrdf::Triple> =
        parse_triples(&pre_nt)?.into_iter().collect();
    for triple in &removed {
        post.remove(triple);
    }
    post.extend(parse_triples(asserted_nt)?.into_iter().filter(
        |t| matches!(&t.subject, NamedOrBlankNode::NamedNode(s) if subjects.contains(s.as_str())),
    ));
    let post_nt: String = post.iter().map(|t| format!("{t} .\n")).collect();

    let before = crate::shacl::validate_shapes(shapes_turtle, &pre_nt)?;
    let debt: BTreeSet<String> = before
        .results
        .iter()
        .filter(|r| is_violation(r))
        .map(debt_key)
        .collect();
    let after = crate::shacl::validate_shapes(shapes_turtle, &post_nt)?;
    feedback.results = after
        .results
        .into_iter()
        // A delete can only take a value away, so only a floor can newly fail.
        // Other components are not judged on this bounded post-state.
        .filter(|r| is_violation(r) && subjects.contains(&r.focus_node))
        .filter(|r| r.component.ends_with("MinCountConstraintComponent"))
        .filter(|r| !debt.contains(&debt_key(r)))
        .collect();
    feedback.violations = feedback.results.len();
    feedback.conforms = feedback.violations == 0;
    Ok(feedback)
}

#[cfg(feature = "shacl")]
fn parse_triples(nt: &str) -> Result<Vec<oxrdf::Triple>> {
    RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(nt.as_bytes())
        .map(|quad| {
            quad.map(oxrdf::Triple::from)
                .map_err(|e| Error::InvalidValue(format!("RDF parse error: {e}")))
        })
        .collect()
}

/// Every current fact of `iris` in graph `g` and ROOT, plus the types of every
/// IRI those facts reference, as N-Triples: the pre-state debt is judged on.
#[cfg(feature = "shacl")]
fn store_description_in_graph(store: &Store, iris: &BTreeSet<String>, g: i64) -> Result<String> {
    let mut graphs = vec![crate::schema::ROOT_GRAPH];
    if g != crate::schema::ROOT_GRAPH {
        graphs.push(g);
    }
    let mut lines = BTreeSet::new();
    let mut referenced = BTreeSet::new();
    for iri in iris {
        let Some(entity) = store.lookup(iri)? else {
            continue;
        };
        for &scope in &graphs {
            for fact in store.entity_facts_in_graph(entity, scope)? {
                let attr = store.resolve(fact.attribute)?;
                let Ok(term) = crate::rdf::value_to_term(store, &fact.value) else {
                    continue;
                };
                if let (Value::Ref(id), true) = (&fact.value, attr != RDF_TYPE) {
                    referenced.insert(store.resolve(*id)?);
                }
                lines.insert(format!("<{iri}> <{attr}> {term} .\n"));
            }
        }
    }
    let mut out: String = lines.into_iter().collect();
    out.push_str(&store_type_context_in_graph(store, &referenced, g)?);
    Ok(out)
}

/// Components a node's own description and its references' types decide
/// completely, on a plain predicate path. Anything else is never debt.
#[cfg(feature = "shacl")]
fn is_focus_local(issue: &crate::shacl::ValidationIssue) -> bool {
    const LOCAL: &[&str] = &[
        "MinCountConstraintComponent",
        "MaxCountConstraintComponent",
        "HasValueConstraintComponent",
        "InConstraintComponent",
        "DatatypeConstraintComponent",
        "NodeKindConstraintComponent",
        "PatternConstraintComponent",
        "MinLengthConstraintComponent",
        "MaxLengthConstraintComponent",
        "LanguageInConstraintComponent",
        "ClassConstraintComponent",
    ];
    let component = issue
        .component
        .trim_matches(['<', '>'])
        .trim_start_matches("http://www.w3.org/ns/shacl#");
    let plain_path = issue.path.as_deref().is_some_and(|p| {
        p.starts_with("http://") || p.starts_with("https://") || p.starts_with("urn:")
    });
    LOCAL.contains(&component) && plain_path
}

#[cfg(feature = "shacl")]
fn is_violation(issue: &crate::shacl::ValidationIssue) -> bool {
    issue.severity.to_ascii_lowercase().contains("violation")
}

/// Identity of a violation for debt matching: stricter than [`issue_key`].
///
/// Adds the message, which names the constraint even when the property shape
/// is anonymous, and the source shape when it is an IRI. A blank-node shape id
/// is fresh per run (see [`issue_key`]), so it is left out; omitting it can only
/// make two issues look different, which refuses rather than admits.
#[cfg(feature = "shacl")]
fn debt_key(issue: &crate::shacl::ValidationIssue) -> String {
    let shape = issue
        .source_shape
        .as_deref()
        .filter(|s| !s.starts_with("_:"))
        .unwrap_or("");
    format!(
        "{}|{shape}|{}",
        issue_key(issue),
        issue.message.as_deref().unwrap_or("")
    )
}

/// The baseline's violations MINUS the ones the store context resolved.
///
/// Subtraction, never union: an issue present only in the augmented run is
/// something the CONTEXT introduced (a referenced node becoming a validation
/// target), and reporting it would refuse a write for a fact it does not
/// assert. Those are dropped by construction here — not by a filter that has to
/// be right about targeting.
///
/// Recounted from the kept issues rather than carried over from either report:
/// a result that said `conforms: false, violations: 0` would be a refusal with
/// nothing to show for it.
fn repaired(
    baseline: crate::shacl::ValidationFeedback,
    augmented: &crate::shacl::ValidationFeedback,
) -> crate::shacl::ValidationFeedback {
    let still: BTreeSet<String> = augmented.results.iter().map(issue_key).collect();
    let results: Vec<_> = baseline
        .results
        .into_iter()
        .filter(|issue| still.contains(&issue_key(issue)))
        .collect();
    let violations = results
        .iter()
        .filter(|r| r.severity.to_ascii_lowercase().contains("violation"))
        .count();
    let warnings = results
        .iter()
        .filter(|r| r.severity.to_ascii_lowercase().contains("warning"))
        .count();
    crate::shacl::ValidationFeedback {
        conforms: violations == 0,
        violations,
        warnings,
        results,
        resolution_candidates: baseline.resolution_candidates,
    }
}

/// Identity of a validation issue across two runs of the same shapes.
///
/// Deliberately excludes `source_shape`: rudof labels an anonymous property
/// shape with a fresh blank-node id per run (`_:ad3f1b9a…`), so including it
/// would make every issue unique and the subtraction a no-op — the fix would
/// silently stop repairing anything while every test that checks REFUSALS kept
/// passing.
fn issue_key(issue: &crate::shacl::ValidationIssue) -> String {
    format!(
        "{}|{}|{}|{}",
        issue.focus_node,
        issue.component,
        issue.path.as_deref().unwrap_or(""),
        issue.value.as_deref().unwrap_or(""),
    )
}

#[cfg(test)]
#[path = "shacl_context_tests.rs"]
mod shacl_context_tests;

#[cfg(all(test, feature = "shacl"))]
#[path = "shacl_objects_of_tests.rs"]
mod shacl_objects_of_tests;

#[cfg(all(test, feature = "shacl"))]
#[path = "shacl_debt_tests.rs"]
mod shacl_debt_tests;

/// Queue each result of an EMIT-mode validation as a `shacl.violation` event
/// riding the next write's savepoint (event P3). One definition for every
/// write path that routes by `quipu:onViolation` (`/episode`, `/knot`), so the
/// event shape cannot drift between them (aegis-4c3ppi).
pub fn queue_emit_violations(store: &Store, feedback: &crate::shacl::ValidationFeedback) {
    // Every result, not only blocking ones: an emit shape may declare
    // sh:Warning (aegis-1mv0to), and then `conforms` is true while there is
    // still something to observe.
    if feedback.results.is_empty() {
        return;
    }
    for issue in &feedback.results {
        store.queue_write_event(crate::store::PendingWriteEvent {
            event_type: "shacl.violation".to_string(),
            subject: Some(issue.focus_node.clone()),
            payload: serde_json::json!({
                "shape": issue.source_shape,
                "message": issue.message,
                "component": issue.component,
                "path": issue.path,
                "severity": issue.severity,
                "mode": "emit",
            }),
        });
    }
}
