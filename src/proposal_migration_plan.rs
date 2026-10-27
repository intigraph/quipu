//! Evaluating a migration program into a net change set, read-only.
//!
//! Each template's `WHERE` runs as a SELECT through quipu's own SPARQL engine
//! over the ROOT graph; both templates are instantiated over every solution
//! and every operation reads the pre-migration state (the SPARQL 1.1 Update
//! §3.1.3 order, `(before − deletes) ∪ inserts`, extended to the program).

use std::collections::{BTreeMap, HashMap};

use spargebra::GraphUpdateOperation;
use spargebra::term::{
    GraphNamePattern, GroundQuadPattern, GroundTermPattern, NamedNodePattern, QuadPattern,
    TermPattern,
};

use super::MigrationOp;
use crate::error::{Error, Result};
use crate::store::{Datum, Store};
use crate::types::{Op, Value};

/// An RDF object as a migration writes it: an IRI (interned only when the
/// write commits) or a literal value.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Obj {
    Iri(String),
    Lit(Value),
}

/// One planned triple in ROOT.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Planned {
    pub(super) subject: String,
    pub(super) predicate: String,
    pub(super) object: Obj,
}

type Key = (String, String, Vec<u8>);

impl Planned {
    fn key(&self) -> Key {
        let object = match &self.object {
            Obj::Iri(iri) => [&[0xff_u8][..], iri.as_bytes()].concat(),
            Obj::Lit(value) => value.term_key(),
        };
        (self.subject.clone(), self.predicate.clone(), object)
    }

    fn term(iri: &str) -> String {
        if iri.starts_with("_:") {
            iri.to_owned()
        } else {
            format!("<{iri}>")
        }
    }

    /// The triple as one N-Triples line (what the write gate judges).
    pub(super) fn n_triple(&self, store: &Store) -> Result<String> {
        let object = match &self.object {
            Obj::Iri(iri) => Self::term(iri),
            Obj::Lit(value) => crate::rdf::value_to_term(store, value)?.to_string(),
        };
        Ok(format!(
            "{} {} {object} .\n",
            Self::term(&self.subject),
            Self::term(&self.predicate)
        ))
    }
}

/// What one operation of the program changes, after netting.
#[derive(Debug, Clone, serde::Serialize)]
pub struct OpReport {
    pub index: usize,
    pub op: &'static str,
    /// The SPARQL Update the operation ran.
    pub update: String,
    /// Existing facts the operation supersedes.
    pub retracted: usize,
    /// New facts the operation writes.
    pub asserted: usize,
}

/// The net change a migration program makes to the current ROOT graph.
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    pub ops: Vec<OpReport>,
    /// Existing facts to supersede, each with its stored value.
    pub(super) retract: Vec<Planned>,
    /// Facts to write.
    pub(super) assert: Vec<Planned>,
}

impl MigrationPlan {
    /// How many existing facts the migration supersedes.
    #[must_use]
    pub fn retract_count(&self) -> usize {
        self.retract.len()
    }

    /// How many facts the migration writes.
    #[must_use]
    pub fn assert_count(&self) -> usize {
        self.assert.len()
    }

    /// The retracted and asserted triples as N-Triples, for the write gate.
    pub(super) fn n_triples(&self, store: &Store) -> Result<(String, String)> {
        let mut retracted = String::new();
        for p in &self.retract {
            retracted.push_str(&p.n_triple(store)?);
        }
        let mut asserted = String::new();
        for p in &self.assert {
            asserted.push_str(&p.n_triple(store)?);
        }
        Ok((asserted, retracted))
    }

    /// The datums that supersede the old facts and write the new ones at
    /// `timestamp`: a retraction closes the old fact's valid time, it never
    /// rewrites it.
    pub(super) fn datums(&self, store: &Store, timestamp: &str) -> Result<Vec<Datum>> {
        let mut datums = Vec::with_capacity(self.retract.len() + self.assert.len());
        for (planned, op) in self
            .retract
            .iter()
            .map(|p| (p, Op::Retract))
            .chain(self.assert.iter().map(|p| (p, Op::Assert)))
        {
            let value = match &planned.object {
                Obj::Iri(iri) => Value::Ref(store.intern(iri)?),
                Obj::Lit(value) => value.clone(),
            };
            datums.push(Datum {
                entity: store.intern(&planned.subject)?,
                attribute: store.intern(&planned.predicate)?,
                value,
                valid_from: timestamp.to_owned(),
                valid_to: (op == Op::Retract).then(|| timestamp.to_owned()),
                op,
            });
        }
        Ok(datums)
    }
}

fn template_error(message: impl std::fmt::Display) -> Error {
    Error::InvalidValue(format!("migration template: {message}"))
}

fn parse(update: &str) -> Result<spargebra::Update> {
    crate::sparql_structure::parse_update(crate::sparql::sparql_parser(), update)?
        .map_err(template_error)
}

/// Refuse a template quipu cannot run as a ROOT-graph migration.
pub(super) fn check_template(update: &str) -> Result<()> {
    for operation in parse(update)?.operations {
        match operation {
            GraphUpdateOperation::DeleteInsert {
                delete,
                insert,
                using,
                ..
            } => {
                if using.is_some() {
                    return Err(template_error("USING / WITH is not supported"));
                }
                if delete
                    .iter()
                    .any(|q| q.graph_name != GraphNamePattern::DefaultGraph)
                    || insert
                        .iter()
                        .any(|q| q.graph_name != GraphNamePattern::DefaultGraph)
                {
                    return Err(template_error("templates write the ROOT graph only"));
                }
                if insert.iter().any(has_blank_node) {
                    return Err(template_error("blank nodes in INSERT are not supported"));
                }
            }
            GraphUpdateOperation::InsertData { data } => {
                if data
                    .iter()
                    .any(|q| q.graph_name != spargebra::term::GraphName::DefaultGraph)
                {
                    return Err(template_error("templates write the ROOT graph only"));
                }
                if data.iter().any(|q| {
                    matches!(q.subject, oxrdf::NamedOrBlankNode::BlankNode(_))
                        || matches!(q.object, oxrdf::Term::BlankNode(_))
                }) {
                    return Err(template_error("blank nodes in INSERT are not supported"));
                }
            }
            GraphUpdateOperation::DeleteData { data } => {
                if data
                    .iter()
                    .any(|q| q.graph_name != spargebra::term::GraphName::DefaultGraph)
                {
                    return Err(template_error("templates write the ROOT graph only"));
                }
            }
            _ => {
                return Err(template_error(
                    "only DELETE/INSERT … WHERE, INSERT DATA and DELETE DATA are supported",
                ));
            }
        }
    }
    Ok(())
}

fn has_blank_node(q: &QuadPattern) -> bool {
    matches!(q.subject, TermPattern::BlankNode(_)) || matches!(q.object, TermPattern::BlankNode(_))
}

/// Evaluate a migration program against the current ROOT graph.
pub fn plan(store: &Store, program: &[MigrationOp]) -> Result<MigrationPlan> {
    let mut deletes: BTreeMap<Key, Planned> = BTreeMap::new();
    let mut inserts: BTreeMap<Key, Planned> = BTreeMap::new();
    let mut per_op: Vec<(Vec<Key>, Vec<Key>)> = Vec::new();
    let mut ops = Vec::new();
    for (index, op) in program.iter().enumerate() {
        let context =
            |e: Error| Error::InvalidValue(format!("migration op {index} ({}): {e}", op.name()));
        let update = op.render().map_err(context)?;
        check_template(&update).map_err(context)?;
        let (d, i) = evaluate(store, &update).map_err(context)?;
        for p in &i {
            check_literal(p).map_err(context)?;
        }
        per_op.push((
            d.iter().map(Planned::key).collect(),
            i.iter().map(Planned::key).collect(),
        ));
        deletes.extend(d.into_iter().map(|p| (p.key(), p)));
        inserts.extend(i.into_iter().map(|p| (p.key(), p)));
        ops.push(OpReport {
            index,
            op: op.name(),
            update,
            retracted: 0,
            asserted: 0,
        });
    }
    let mut existing = Existing::default();
    let mut retract = BTreeMap::new();
    for (key, planned) in &deletes {
        if inserts.contains_key(key) {
            continue;
        }
        if let Some(stored) = existing.get(store, planned)? {
            retract.insert(
                key.clone(),
                Planned {
                    object: Obj::Lit(stored),
                    ..planned.clone()
                },
            );
        }
    }
    let mut assert = BTreeMap::new();
    for (key, planned) in inserts {
        if existing.get(store, &planned)?.is_none() {
            assert.insert(key, planned);
        }
    }
    for (report, (d, i)) in ops.iter_mut().zip(&per_op) {
        report.retracted = d.iter().filter(|k| retract.contains_key(*k)).count();
        report.asserted = i.iter().filter(|k| assert.contains_key(*k)).count();
    }
    Ok(MigrationPlan {
        ops,
        retract: retract.into_values().collect(),
        assert: assert.into_values().collect(),
    })
}

/// Current ROOT facts per subject, keyed by `(attribute, term key)`, read once.
#[derive(Default)]
struct Existing(HashMap<String, HashMap<(i64, Vec<u8>), Value>>);

impl Existing {
    /// The stored value of `planned`, when it is a current ROOT fact.
    fn get(&mut self, store: &Store, planned: &Planned) -> Result<Option<Value>> {
        let Some(attribute) = store.lookup(&planned.predicate)? else {
            return Ok(None);
        };
        let key = match &planned.object {
            Obj::Iri(iri) => match store.lookup(iri)? {
                Some(id) => Value::Ref(id).term_key(),
                None => return Ok(None),
            },
            Obj::Lit(value) => value.term_key(),
        };
        if !self.0.contains_key(&planned.subject) {
            let mut facts = HashMap::new();
            if let Some(entity) = store.lookup(&planned.subject)? {
                for fact in store.entity_facts_in_graph(entity, crate::schema::ROOT_GRAPH)? {
                    facts.insert((fact.attribute, fact.value.term_key()), fact.value);
                }
            }
            self.0.insert(planned.subject.clone(), facts);
        }
        Ok(self.0[&planned.subject].get(&(attribute, key)).cloned())
    }
}

/// The deletes and inserts of every operation in `update`, each over the
/// pre-migration state.
fn evaluate(store: &Store, update: &str) -> Result<(Vec<Planned>, Vec<Planned>)> {
    let parsed = parse(update)?;
    let (mut deletes, mut inserts) = (Vec::new(), Vec::new());
    for operation in parsed.operations {
        match operation {
            GraphUpdateOperation::DeleteInsert {
                delete,
                insert,
                pattern,
                ..
            } => {
                let select = spargebra::Query::Select {
                    dataset: None,
                    pattern: *pattern,
                    base_iri: parsed.base_iri.clone(),
                };
                let result = crate::sparql::query(store, &select.to_string())?;
                for row in result.rows() {
                    for q in &delete {
                        if let Some(p) = ground(store, q, row)? {
                            deletes.push(p);
                        }
                    }
                    for q in &insert {
                        if let Some(p) = instantiate(store, q, row)? {
                            inserts.push(p);
                        }
                    }
                }
            }
            GraphUpdateOperation::InsertData { data } => {
                for q in data {
                    inserts.push(Planned {
                        subject: match &q.subject {
                            oxrdf::NamedOrBlankNode::NamedNode(n) => n.as_str().to_owned(),
                            blank @ oxrdf::NamedOrBlankNode::BlankNode(_) => blank.to_string(),
                        },
                        predicate: q.predicate.as_str().to_owned(),
                        object: constant_object(&q.object),
                    });
                }
            }
            GraphUpdateOperation::DeleteData { data } => {
                for q in data {
                    deletes.push(Planned {
                        subject: q.subject.as_str().to_owned(),
                        predicate: q.predicate.as_str().to_owned(),
                        object: match &q.object {
                            spargebra::term::GroundTerm::NamedNode(n) => {
                                Obj::Iri(n.as_str().to_owned())
                            }
                            spargebra::term::GroundTerm::Literal(l) => {
                                Obj::Lit(crate::literal_identity::literal_to_value(l))
                            }
                            #[cfg(feature = "shacl")]
                            spargebra::term::GroundTerm::Triple(_) => continue,
                        },
                    });
                }
            }
            _ => return Err(template_error("unsupported operation")),
        }
    }
    Ok((deletes, inserts))
}

// The wildcard also covers quoted triples, which exist only with `shacl` (RDF 1.2).
#[allow(clippy::match_wildcard_for_single_variants)]
fn constant_object(term: &oxrdf::Term) -> Obj {
    match term {
        oxrdf::Term::NamedNode(n) => Obj::Iri(n.as_str().to_owned()),
        oxrdf::Term::Literal(l) => Obj::Lit(crate::literal_identity::literal_to_value(l)),
        other => Obj::Iri(other.to_string()),
    }
}

/// A bound IRI as a string; anything else cannot be a subject or predicate.
fn bound_iri(store: &Store, row: &crate::sparql::Bindings, var: &str) -> Result<Option<String>> {
    Ok(match row.get(var) {
        Some(Value::Ref(id)) => Some(store.resolve(*id)?),
        _ => None,
    })
}

/// A bound object: IRIs normalised to [`Obj::Iri`], typed literals to their
/// canonical value (so `STRDT("42", xsd:integer)` writes the integer 42).
fn bound_object(store: &Store, value: &Value) -> Result<Obj> {
    Ok(match value {
        Value::Ref(id) => Obj::Iri(store.resolve(*id)?),
        Value::Typed { lexical, datatype } => {
            let datatype = oxrdf::NamedNode::new(datatype.as_str()).map_err(template_error)?;
            Obj::Lit(crate::literal_identity::literal_to_value(
                &oxrdf::Literal::new_typed_literal(lexical.as_str(), datatype),
            ))
        }
        other => Obj::Lit(other.clone()),
    })
}

fn subject_of(
    store: &Store,
    pattern: &TermPattern,
    row: &crate::sparql::Bindings,
) -> Result<Option<String>> {
    Ok(match pattern {
        TermPattern::NamedNode(n) => Some(n.as_str().to_owned()),
        TermPattern::Variable(v) => bound_iri(store, row, v.as_str())?,
        _ => None,
    })
}

fn predicate_of(
    store: &Store,
    pattern: &NamedNodePattern,
    row: &crate::sparql::Bindings,
) -> Result<Option<String>> {
    Ok(match pattern {
        NamedNodePattern::NamedNode(n) => Some(n.as_str().to_owned()),
        NamedNodePattern::Variable(v) => bound_iri(store, row, v.as_str())?,
    })
}

// The wildcard also covers quoted triples, which exist only with `shacl` (RDF 1.2).
#[allow(clippy::match_wildcard_for_single_variants)]
fn object_of(
    store: &Store,
    pattern: &TermPattern,
    row: &crate::sparql::Bindings,
) -> Result<Option<Obj>> {
    Ok(match pattern {
        TermPattern::NamedNode(n) => Some(Obj::Iri(n.as_str().to_owned())),
        TermPattern::Literal(l) => Some(Obj::Lit(crate::literal_identity::literal_to_value(l))),
        TermPattern::Variable(v) => match row.get(v.as_str()) {
            Some(value) => Some(bound_object(store, value)?),
            None => None,
        },
        _ => None,
    })
}

fn instantiate(
    store: &Store,
    q: &QuadPattern,
    row: &crate::sparql::Bindings,
) -> Result<Option<Planned>> {
    let (Some(subject), Some(predicate), Some(object)) = (
        subject_of(store, &q.subject, row)?,
        predicate_of(store, &q.predicate, row)?,
        object_of(store, &q.object, row)?,
    ) else {
        return Ok(None);
    };
    Ok(Some(Planned {
        subject,
        predicate,
        object,
    }))
}

fn ground_pattern(p: &GroundTermPattern) -> TermPattern {
    match p {
        GroundTermPattern::NamedNode(n) => TermPattern::NamedNode(n.clone()),
        GroundTermPattern::Literal(l) => TermPattern::Literal(l.clone()),
        GroundTermPattern::Variable(v) => TermPattern::Variable(v.clone()),
        // A quoted triple never matches a stored fact: an unbound variable
        // makes the template quad drop out.
        #[cfg(feature = "shacl")]
        GroundTermPattern::Triple(_) => {
            TermPattern::Variable(spargebra::term::Variable::new_unchecked("__unsupported"))
        }
    }
}

fn ground(
    store: &Store,
    q: &GroundQuadPattern,
    row: &crate::sparql::Bindings,
) -> Result<Option<Planned>> {
    let (Some(subject), Some(predicate), Some(object)) = (
        subject_of(store, &ground_pattern(&q.subject), row)?,
        predicate_of(store, &q.predicate, row)?,
        object_of(store, &ground_pattern(&q.object), row)?,
    ) else {
        return Ok(None);
    };
    Ok(Some(Planned {
        subject,
        predicate,
        object,
    }))
}

/// Refuse a migration that would write an ill-typed XSD literal — a cast of
/// `"abc"` to `xsd:integer` fails the whole program instead of storing it.
fn check_literal(planned: &Planned) -> Result<()> {
    let Obj::Lit(Value::Typed { lexical, datatype }) = &planned.object else {
        return Ok(());
    };
    if super::lexical::well_typed(lexical, datatype) {
        return Ok(());
    }
    Err(Error::InvalidValue(format!(
        "would write the ill-typed literal \"{lexical}\"^^<{datatype}> for <{}> <{}>; \
         no facts were written",
        planned.subject, planned.predicate
    )))
}
