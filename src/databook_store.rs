//! Export a graph as a DataBook, and import a DataBook's graph data.
//!
//! Export writes one graph (ROOT or a named graph), current or as of a past
//! transaction/valid time, as canonical Turtle, optionally with the stored
//! shapes as `shacl` blocks. Every block carries a `quipu:sha256` annotation
//! over its payload, which import verifies before it writes anything.
//!
//! Import loads `turtle`, `turtle12`, `n-triples`, `trig` and `n-quads`
//! blocks, honouring TriG/N-Quads graph names, in ONE all-or-nothing write
//! across graphs ([`Store::transact_graph_batches_tx`]). It never executes a
//! block: `sparql` and `sparql-update` are reported as skipped, and `shacl`
//! blocks load only when asked, after the data commits. Blank nodes are
//! scoped to their block (`BlockNodes`).

use std::collections::{BTreeMap, BTreeSet, HashMap};

use oxrdf::{BlankNode, GraphName, NamedOrBlankNode, Quad, Term, Triple};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};
use sha2::{Digest, Sha256};

use crate::databook::{self, Block, BlockKind, DataBook, Frontmatter};
use crate::error::{Error, Result};
use crate::schema::ROOT_GRAPH_IRI;
use crate::store::{AsOf, Datum, Store};

/// The fixed IRI the process stamp names as the transformer.
pub const TRANSFORMER_IRI: &str = "https://github.com/intigraph/quipu";

/// What to export.
pub struct ExportOptions<'a> {
    /// The graph IRI; `None` (or [`ROOT_GRAPH_IRI`]) is ROOT.
    pub graph: Option<&'a str>,
    /// Export the graph as it stood at this point instead of now.
    pub as_of: Option<AsOf>,
    /// The DataBook `id`; defaults to a content-addressed URN.
    pub id: Option<&'a str>,
    pub title: Option<&'a str>,
    pub version: &'a str,
    /// Add each stored shape set (as of the same point) as a `shacl` block.
    pub with_shapes: bool,
    /// RFC 3339 production time; `created` is its date.
    pub timestamp: &'a str,
}

/// What an import wrote and what it left alone.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub id: String,
    pub version: String,
    /// `(graph IRI, transaction, facts)` in commit order.
    pub graphs: Vec<(String, i64, usize)>,
    /// Shape sets loaded, by name.
    pub shapes: Vec<String>,
    /// Blocks not loaded, each with the reason.
    pub skipped: Vec<String>,
    pub warnings: Vec<String>,
}

/// How to import.
#[derive(Debug)]
pub struct ImportOptions<'a> {
    /// Load every graph block into this graph instead of the one it names.
    pub graph: Option<&'a str>,
    /// Load `shacl` blocks as stored shapes (after the data commits).
    pub with_shapes: bool,
    pub timestamp: &'a str,
    pub actor: Option<&'a str>,
    /// Transaction source; defaults to `databook:<id>@<version>`.
    pub source: Option<&'a str>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::InvalidValue(format!("databook: {}", message.into()))
}

fn sha256(text: &str) -> String {
    hex::encode(Sha256::digest(text.trim_end_matches('\n').as_bytes()))
}

fn graph_id(store: &Store, iri: &str) -> Result<i64> {
    if iri == ROOT_GRAPH_IRI {
        return Ok(0);
    }
    store
        .lookup(iri)?
        .ok_or_else(|| invalid(format!("unknown graph: {iri}")))
}

/// The graph's triples, canonical and deduplicated.
fn graph_triples(store: &Store, graph: i64, as_of: Option<&AsOf>) -> Result<Vec<Triple>> {
    let facts = match as_of {
        Some(as_of) => store.facts_as_of_in_graph(as_of, graph)?,
        None => store.current_facts_in_graph(graph)?,
    };
    let nt = crate::rdf::serialize_facts(store, &facts, RdfFormat::NTriples)?;
    RdfParser::from_format(RdfFormat::NTriples)
        .for_reader(nt.as_slice())
        .map(|q| {
            q.map(Triple::from)
                .map_err(|e| invalid(format!("re-reading export: {e}")))
        })
        .collect()
}

/// Kebab-case block id for a shape-set name.
fn block_id(prefix: &str, name: &str) -> String {
    let mut id = String::from(prefix);
    for c in name.chars() {
        id.push(if c.is_ascii_alphanumeric() {
            c.to_ascii_lowercase()
        } else {
            '-'
        });
    }
    id
}

/// Export a graph as a DataBook document.
pub fn export(store: &Store, opts: &ExportOptions<'_>) -> Result<String> {
    let graph_iri = opts.graph.unwrap_or(ROOT_GRAPH_IRI);
    let graph = graph_id(store, graph_iri)?;
    let triples = graph_triples(store, graph, opts.as_of.as_ref())?;
    let subjects: BTreeSet<String> = triples.iter().map(|t| t.subject.to_string()).collect();
    let turtle = String::from_utf8(crate::rdf_export::serialize_triples_canonical(
        &triples,
        RdfFormat::Turtle,
    )?)
    .map_err(|e| invalid(format!("Turtle is not UTF-8: {e}")))?;
    let data_hash = sha256(&turtle);
    let id = opts.id.map_or_else(
        || format!("urn:quipu:databook:sha256:{data_hash}"),
        str::to_string,
    );
    let title = opts
        .title
        .map_or_else(|| format!("Quipu graph {graph_iri}"), str::to_string);
    let tx = match &opts.as_of {
        Some(AsOf { tx: Some(tx), .. }) => *tx,
        _ => store.latest_tx_id()?,
    };
    let valid_at = opts.as_of.as_ref().and_then(|a| a.valid_at.as_deref());

    let mut blocks = vec![databook::fence(
        "turtle",
        &[
            ("id", "graph-data"),
            ("graph", graph_iri),
            ("quipu:sha256", &data_hash),
        ],
        &turtle,
    )?];
    if opts.with_shapes {
        let shapes = match &opts.as_of {
            Some(as_of) => store.list_shapes_as_of(as_of)?,
            None => store.list_shapes()?,
        };
        for (name, text, _) in &shapes {
            let id = block_id("shapes-", name);
            let annotations = [
                ("id", id.as_str()),
                ("quipu:shapes-name", name.as_str()),
                ("quipu:sha256", &sha256(text)),
            ];
            blocks.push(
                databook::fence("shacl", &annotations, text)
                    .map_err(|e| invalid(format!("shape set `{name}`: {e}")))?,
            );
        }
    }

    let mut front = Frontmatter::default();
    front
        .text(0, "id", &id)
        .text(0, "title", &title)
        .text(0, "type", "databook")
        .text(0, "version", opts.version)
        .text(
            0,
            "created",
            opts.timestamp.get(..10).unwrap_or(opts.timestamp),
        )
        .text(
            0,
            "description",
            &format!("Graph {graph_iri} exported from a Quipu store."),
        )
        .open(0, "graph")
        .text(1, "named_graph", graph_iri)
        .number(1, "triple_count", triples.len() as u64)
        .number(1, "subjects", subjects.len() as u64)
        .text(1, "rdf_version", "1.1")
        .text(1, "turtle_version", "1.1")
        .open(0, "process")
        .text(
            1,
            "transformer",
            &format!("quipu {}", env!("CARGO_PKG_VERSION")),
        )
        .text(1, "transformer_type", "script")
        .text(1, "transformer_iri", TRANSFORMER_IRI)
        .open(1, "inputs")
        .item(2, "iri", graph_iri)
        .text(3, "role", "primary")
        .text(1, "timestamp", opts.timestamp)
        .open(0, "quipu")
        .number(1, "tx", u64::try_from(tx).unwrap_or(0));
    if let Some(valid_at) = valid_at {
        front.text(1, "valid_at", valid_at);
    }

    let when = match (&opts.as_of, valid_at) {
        (None, _) => format!("its current facts at transaction {tx}"),
        (Some(_), Some(v)) => format!("its facts as of transaction {tx}, valid at {v}"),
        (Some(_), None) => format!("its facts as of transaction {tx}"),
    };
    let mut out = front.finish();
    out.push_str(&format!(
        "\n# {title}\n\nThe graph `{graph_iri}`: {when}, {} triples about {} subjects, \
         in canonical Turtle.\n\n## Graph data\n\n",
        triples.len(),
        subjects.len()
    ));
    out.push_str(&blocks[0]);
    if blocks.len() > 1 {
        out.push_str("\n## Shapes\n\nThe store's SHACL shape sets, one block per set.\n");
        for block in &blocks[1..] {
            out.push('\n');
            out.push_str(block);
        }
    }
    Ok(out)
}

/// A block's quads, keyed by the graph each should load into.
fn block_quads(block: &Block, format: RdfFormat) -> Result<Vec<Quad>> {
    RdfParser::from_format(format)
        .for_reader(block.payload.as_bytes())
        .map(|q| {
            q.map_err(|e| invalid(format!("{} block at line {}: {e}", block.label, block.line)))
        })
        .collect()
}

/// Blank-node names for one block. They derive from the document's id and
/// version, the block's key and its payload, plus each node's order of first
/// appearance in the block. The key is the block's `databook:id`, or else its
/// position, and its kind is hashed separately: an explicit id spelled like a
/// position (`#1`) cannot stand in for the anonymous block at that position. So
/// importing the same document again gives the same nodes; no two blocks share
/// a node, even with identical payloads; and one `trig` block keeps a node shared
/// across the graphs it names.
struct BlockNodes {
    prefix: String,
    nodes: HashMap<BlankNode, BlankNode>,
}

impl BlockNodes {
    fn new(book: &DataBook, block: &Block, index: usize) -> Self {
        let (kind, key) = match block.id() {
            Some(id) => ("databook:id", id.to_string()),
            None => ("position", index.to_string()),
        };
        let mut hash = Sha256::new();
        // Every part is length-prefixed, so the encoding is injective.
        for part in [
            "quipu-databook-bnodes-v1",
            &book.header.id,
            &book.header.version,
            kind,
            &key,
            &block.payload,
        ] {
            hash.update((part.len() as u64).to_be_bytes());
            hash.update(part.as_bytes());
        }
        Self {
            prefix: format!("d{}", &hex::encode(hash.finalize())[..32]),
            nodes: HashMap::new(),
        }
    }

    fn node(&mut self, node: BlankNode) -> BlankNode {
        let next = self.nodes.len();
        self.nodes
            .entry(node)
            .or_insert_with(|| BlankNode::new_unchecked(format!("{}_{next}", self.prefix)))
            .clone()
    }

    fn triple(&mut self, triple: Triple) -> Triple {
        let subject = match triple.subject {
            NamedOrBlankNode::BlankNode(node) => self.node(node).into(),
            other @ NamedOrBlankNode::NamedNode(_) => other,
        };
        let object = match triple.object {
            Term::BlankNode(node) => self.node(node).into(),
            other => other,
        };
        Triple {
            subject,
            predicate: triple.predicate,
            object,
        }
    }
}

fn describe(block: &Block) -> String {
    match block.id() {
        Some(id) => format!("{} block `{id}` (line {})", block.label, block.line),
        None => format!("{} block at line {}", block.label, block.line),
    }
}

/// Import a DataBook's graph data (and, if asked, its shapes) into `store`.
pub fn import(store: &mut Store, text: &str, opts: &ImportOptions<'_>) -> Result<ImportReport> {
    let book = databook::parse(text)?;
    for block in &book.blocks {
        if let Some(expected) = block.meta.get("quipu:sha256")
            && *expected != sha256(&block.payload)
        {
            return Err(invalid(format!(
                "{} fails its quipu:sha256 check",
                describe(block)
            )));
        }
    }
    let mut report = ImportReport {
        id: book.header.id.clone(),
        version: book.header.version.clone(),
        warnings: book.warnings.clone(),
        ..ImportReport::default()
    };
    // Graph IRI -> one (block, triples) group per contributing block.
    let mut per_graph: BTreeMap<String, Vec<(usize, String, Vec<Triple>)>> = BTreeMap::new();
    let mut shapes = Vec::new();
    let mut triple_count = 0u64;
    for (index, block) in book.blocks.iter().enumerate() {
        let skip = match block.kind() {
            BlockKind::Rdf(format) => {
                let default = opts
                    .graph
                    .or(block.meta.get("graph").map(String::as_str))
                    .or(book.header.named_graph.as_deref())
                    .unwrap_or(ROOT_GRAPH_IRI);
                let mut nodes = BlockNodes::new(&book, block, index);
                for quad in block_quads(block, format)? {
                    let target = match (&quad.graph_name, opts.graph) {
                        (GraphName::NamedNode(g), None) => g.as_str().to_string(),
                        (GraphName::BlankNode(_), None) => {
                            return Err(invalid(format!(
                                "{} names a blank-node graph",
                                describe(block)
                            )));
                        }
                        _ => default.to_string(),
                    };
                    triple_count += 1;
                    let triple = nodes.triple(Triple::from(quad));
                    let entry = per_graph.entry(target).or_default();
                    match entry.last_mut() {
                        Some((i, _, triples)) if *i == index => triples.push(triple),
                        _ => entry.push((index, describe(block), vec![triple])),
                    }
                }
                None
            }
            BlockKind::Shapes if opts.with_shapes => {
                shapes.push(block);
                None
            }
            BlockKind::Shapes => Some("shapes load only with --with-shapes"),
            BlockKind::Query => Some("queries are not executed"),
            BlockKind::Update => Some("sparql-update is never executed by import"),
            BlockKind::Opaque => Some("encrypted, and this reader has no encryption profile"),
            BlockKind::Unsupported => Some("this label is not loaded by Quipu"),
            BlockKind::Display if block.label_kind() != BlockKind::Display => {
                Some("marked for reading only (mode or display-only)")
            }
            BlockKind::Display => None,
        };
        if let Some(reason) = skip {
            report
                .skipped
                .push(format!("{}: {reason}", describe(block)));
        }
    }
    if per_graph.is_empty() && shapes.is_empty() {
        let why = if report.skipped.is_empty() {
            String::new()
        } else {
            format!("; skipped: {}", report.skipped.join("; "))
        };
        return Err(invalid(format!("no graph data to import{why}")));
    }
    // `triple_count` counts the primary data (SPEC §5.3): the graph blocks, or
    // in a shapes-only DataBook its `shacl` blocks.
    let (counted, what) = if per_graph.is_empty() {
        let shacl = book.blocks.iter().filter(|b| b.kind() == BlockKind::Shapes);
        let mut n = 0u64;
        for block in shacl {
            n += block_quads(block, RdfFormat::Turtle)?.len() as u64;
        }
        (n, "shacl blocks")
    } else {
        (triple_count, "graph blocks")
    };
    if let Some(declared) = book.header.triple_count
        && declared != counted
    {
        report.warnings.push(format!(
            "graph.triple_count says {declared}, the {what} hold {counted}"
        ));
    }

    // A graph's id is its IRI's term id; register the graphs only once every
    // block has converted, so a refused document leaves none behind.
    let mut batches = Vec::new();
    let mut names = BTreeMap::new();
    for (iri, groups) in &per_graph {
        let graph = if iri == ROOT_GRAPH_IRI {
            0
        } else {
            store.intern(iri)?
        };
        names.insert(graph, iri.clone());
        let mut datums: Vec<Datum> = Vec::new();
        for (_, block, triples) in groups {
            let mut nt = RdfSerializer::from_format(RdfFormat::NTriples).for_writer(Vec::new());
            for triple in triples {
                nt.serialize_triple(triple)
                    .map_err(|e| invalid(format!("re-serializing graph data: {e}")))?;
            }
            let nt = nt
                .finish()
                .map_err(|e| invalid(format!("re-serializing graph data: {e}")))?;
            datums.extend(
                // Blank nodes are already named per block (`BlockNodes`).
                crate::rdf::parse_rdf(
                    store,
                    nt.as_slice(),
                    RdfFormat::NTriples,
                    None,
                    opts.timestamp,
                )
                .map_err(|e| invalid(format!("{block} cannot be stored in graph {iri}: {e}")))?,
            );
        }
        batches.push((graph, datums));
    }
    let source = opts.source.map_or_else(
        || format!("databook:{}@{}", book.header.id, book.header.version),
        str::to_string,
    );
    for iri in per_graph.keys().filter(|iri| *iri != ROOT_GRAPH_IRI) {
        store.graph_create(iri)?;
    }
    if !batches.is_empty() {
        let txs =
            store.transact_graph_batches_tx(&batches, opts.timestamp, opts.actor, Some(&source))?;
        for ((graph, tx), (_, datums)) in txs.into_iter().zip(&batches) {
            report
                .graphs
                .push((names[&graph].clone(), tx, datums.len()));
        }
    }
    for block in shapes {
        let name = block
            .meta
            .get("quipu:shapes-name")
            .or_else(|| block.meta.get("id"))
            .cloned()
            .unwrap_or_else(|| format!("databook-line-{}", block.line));
        // A fence holds whole lines; the text they came from ended with one.
        let text = format!("{}\n", block.payload);
        store
            .load_shapes(&name, &text, opts.timestamp)
            .map_err(|e| {
                invalid(format!(
                    "{} (graph data already committed): {e}",
                    describe(block)
                ))
            })?;
        report.shapes.push(name);
    }
    Ok(report)
}

#[cfg(test)]
#[path = "databook_store_tests.rs"]
mod tests;
