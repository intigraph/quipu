//! Change-sets against a live store: `GET /diff` and the `/patch` state check
//! (aegis-4mk). The format itself is [`crate::changeset`]; spec in
//! `docs/design/changeset-format.md`.

use std::collections::{BTreeMap, BTreeSet};

use crate::changeset::{ChangeSet, Header, Statement};
use crate::error::{Error, Result};
use crate::store::{AsOf, Store};
use crate::types::Fact;

/// One side of a diff.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffSide {
    /// ROOT as of a transaction (`tx:N`, or a bare integer).
    Tx(i64),
    /// ROOT now (`main`, `root` or `head`).
    Main,
    /// A fork's graph now (`fork:<name>`).
    Fork(String),
}

impl DiffSide {
    /// Parse `tx:N`, `N`, `main`/`root`/`head`, or `fork:<name>`.
    ///
    /// # Errors
    /// Anything else, or a negative tx.
    pub fn parse(side: &str) -> Result<Self> {
        let side = side.trim();
        if ["main", "root", "head"]
            .iter()
            .any(|s| side.eq_ignore_ascii_case(s))
        {
            return Ok(Self::Main);
        }
        if let Some(name) = side.strip_prefix("fork:") {
            return Ok(Self::Fork(name.to_string()));
        }
        let tx = side.strip_prefix("tx:").unwrap_or(side);
        match tx.parse::<i64>() {
            Ok(n) if n >= 0 => Ok(Self::Tx(n)),
            _ => Err(Error::InvalidValue(format!(
                "diff side must be tx:N, main, or fork:<name>; got {side:?}"
            ))),
        }
    }
}

/// Render a stored fact as a statement, with literals in canonical form.
///
/// # Errors
/// A term cannot be resolved or is not a valid RDF term.
pub fn fact_statement(store: &Store, fact: &Fact, graph: Option<&str>) -> Result<Statement> {
    let quad = oxrdf::Quad::new(
        crate::rdf::id_to_subject(store, fact.entity)?,
        crate::rdf::id_to_predicate(store, fact.attribute)?,
        crate::rdf::value_to_term(store, &fact.value)?,
        graph_name(graph)?,
    );
    Ok(Statement::from_quad(&quad))
}

fn graph_name(graph: Option<&str>) -> Result<oxrdf::GraphName> {
    Ok(match graph {
        Some(g) => oxrdf::GraphName::NamedNode(
            oxrdf::NamedNode::new(g).map_err(|e| Error::InvalidValue(format!("graph IRI: {e}")))?,
        ),
        None => oxrdf::GraphName::DefaultGraph,
    })
}

/// A statement as the store would render it: a literal object is mapped
/// through the store's value model, so `"05"^^xsd:integer` and
/// `"5"^^xsd:integer` compare equal exactly when the store stores them as one.
///
/// # Errors
/// The statement is not a valid quad.
pub fn canonical(store: &Store, statement: &Statement) -> Result<Statement> {
    let mut quad = statement.to_quad()?;
    if let oxrdf::Term::Literal(lit) = &quad.object {
        let value = crate::literal_identity::literal_to_value(lit);
        quad.object = crate::rdf::value_to_term(store, &value)?;
    }
    Ok(Statement::from_quad(&quad))
}

fn refuse_blank(statements: &BTreeSet<Statement>, what: &str) -> Result<()> {
    match statements.iter().find(|s| s.has_blank_node()) {
        Some(s) => Err(Error::InvalidValue(format!(
            "{what} contains a blank node ({}); skolemize it first, a re-applied blank \
             node mints a new node so the change could not be idempotent",
            s.nquad()
        ))),
        None => Ok(()),
    }
}

fn side_facts(store: &Store, side: &DiffSide) -> Result<Vec<Fact>> {
    match side {
        DiffSide::Main => store.current_facts(),
        DiffSide::Tx(tx) => store.facts_as_of(&AsOf {
            tx: Some(*tx),
            valid_at: None,
        }),
        DiffSide::Fork(name) => store.current_facts_in_graph(store.fork_graph_for_read(name)?),
    }
}

/// A side's statements, rendered into the default graph. Forks are lineages
/// of ROOT, so a fork diff reads as a change to ROOT.
///
/// # Errors
/// Unknown fork, unresolvable term, or a blank node.
pub fn side_statements(store: &Store, side: &DiffSide) -> Result<BTreeSet<Statement>> {
    let set = side_facts(store, side)?
        .iter()
        .map(|f| fact_statement(store, f, None))
        .collect::<Result<BTreeSet<_>>>()?;
    refuse_blank(&set, "diff side")?;
    Ok(set)
}

/// `GET /diff`: the change-set that turns `from` into `to`.
///
/// # Errors
/// See [`side_statements`]; also a tx beyond the store's head.
pub fn diff(store: &Store, from: &DiffSide, to: &DiffSide) -> Result<ChangeSet> {
    let head = store.latest_tx_id()?;
    let at = |side: &DiffSide| -> Result<i64> {
        match side {
            DiffSide::Tx(tx) if *tx > head => Err(Error::InvalidValue(format!(
                "tx {tx} is beyond the store head {head}"
            ))),
            DiffSide::Tx(tx) => Ok(*tx),
            _ => Ok(head),
        }
    };
    let (base_tx, target_tx) = (at(from)?, at(to)?);
    let label = |side: &DiffSide| match side {
        DiffSide::Tx(tx) => format!("tx:{tx}"),
        DiffSide::Main => "main".to_string(),
        DiffSide::Fork(name) => format!("fork:{name}"),
    };
    let header = Header {
        base_tx: Some(base_tx),
        target_tx: Some(target_tx),
        store_id: Some(store.store_id()?),
        source: Some(format!("diff:{}..{}", label(from), label(to))),
        shapes_hash: Some(crate::asof_share::shapes_hash(store, base_tx)?),
        ..Header::default()
    };
    Ok(ChangeSet::from_states(
        header,
        side_statements(store, from)?,
        side_statements(store, to)?,
    ))
}

/// What applying a change-set to the store's current state would mean.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PatchState {
    /// Every delete is absent and every insert present: nothing to write.
    AlreadyApplied,
    /// Every delete is present and every insert absent: apply it.
    Applicable,
    /// Mixed: some statements are already in their post-state.
    Conflict {
        /// Deletes that are already absent.
        already_deleted: Vec<Statement>,
        /// Inserts that are already present.
        already_inserted: Vec<Statement>,
    },
}

/// The current statements for each touched `(subject, graph)` pair.
fn touched_state(store: &Store, cs: &ChangeSet) -> Result<BTreeSet<Statement>> {
    let mut pairs: BTreeMap<(String, Option<String>), ()> = BTreeMap::new();
    for s in cs.deletes.iter().chain(&cs.inserts) {
        let quad = s.to_quad()?;
        let subject = match &quad.subject {
            oxrdf::NamedOrBlankNode::NamedNode(n) => n.as_str().to_string(),
            oxrdf::NamedOrBlankNode::BlankNode(b) => format!("_:{}", b.as_str()),
        };
        pairs.insert((subject, s.graph.clone()), ());
    }
    let mut out = BTreeSet::new();
    for (subject, graph) in pairs.keys() {
        let g = match graph {
            None => crate::schema::ROOT_GRAPH,
            Some(iri) => match store.lookup(iri)? {
                Some(g) => g,
                None => continue,
            },
        };
        for entity in store.lookup_all(subject)? {
            for fact in store.entity_facts_in_graph(entity, g)? {
                out.insert(fact_statement(store, &fact, graph.as_deref())?);
            }
        }
    }
    Ok(out)
}

/// Compare a change-set's touched statements with the store's current state.
///
/// # Errors
/// A statement is malformed or carries a blank node.
pub fn classify(store: &Store, cs: &ChangeSet) -> Result<PatchState> {
    let all: BTreeSet<Statement> = cs.deletes.iter().chain(&cs.inserts).cloned().collect();
    refuse_blank(&all, "changeset")?;
    let current = touched_state(store, cs)?;
    let present = |s: &Statement| -> Result<bool> { Ok(current.contains(&canonical(store, s)?)) };
    let mut already_deleted = Vec::new();
    for s in &cs.deletes {
        if !present(s)? {
            already_deleted.push(s.clone());
        }
    }
    let mut already_inserted = Vec::new();
    for s in &cs.inserts {
        if present(s)? {
            already_inserted.push(s.clone());
        }
    }
    Ok(
        match (
            already_deleted.len() == cs.deletes.len() && already_inserted.len() == cs.inserts.len(),
            already_deleted.is_empty() && already_inserted.is_empty(),
        ) {
            // An empty change-set is trivially already applied.
            (true, _) => PatchState::AlreadyApplied,
            (false, true) => PatchState::Applicable,
            (false, false) => PatchState::Conflict {
                already_deleted,
                already_inserted,
            },
        },
    )
}

#[cfg(test)]
#[path = "changeset_store_tests.rs"]
mod tests;
