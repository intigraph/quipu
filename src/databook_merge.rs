//! Store-free, graph-separated DataBook merging on the pendant projection.
use super::{Decisions, invalid, io};
use crate::error::Result;
use crate::share_merge::Graph;
use oxrdf::{GraphName, Quad, Triple};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
#[path = "databook_merge_document.rs"]
mod document;
#[path = "databook_merge_git.rs"]
mod git;
use document::{Block, Document};
pub use git::{check, driver, is_document, load_path, reconcile, resolve};
pub(crate) use git::{finish, prepare};

pub const SCHEMA: &str = "https://github.com/intigraph/quipu/databook-decisions/v1";
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Group {
    pub graph: String,
    pub decisions: Decisions,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    pub schema: String,
    pub document: String,
    pub commits: [String; 3],
    pub inputs: [String; 3],
    pub blocks: Vec<(String, String, String)>,
    pub shapes_hash: String,
    pub groups: Vec<Group>,
}

pub(crate) struct Plan {
    documents: [Document; 3],
    envelope: String,
    blocks: BTreeMap<String, Block>,
    graphs: BTreeMap<String, Graph>,
    owners: BTreeMap<(String, String, String), String>,
    shapes: String,
    pub file: File,
}

fn slot(quad: &Quad) -> (String, String, String) {
    (
        quad.graph_name.to_string(),
        quad.subject.to_string(),
        quad.predicate.to_string(),
    )
}
fn text(graph: &Graph) -> String {
    let mut lines: Vec<_> = graph.iter().map(|triple| format!("{triple} .\n")).collect();
    lines.sort();
    lines.concat()
}
fn graph_iri(graph: &str) -> Result<oxrdf::NamedNode> {
    oxrdf::NamedNode::new(
        graph
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .ok_or_else(|| invalid("DataBook graph is not an IRI"))?,
    )
    .map_err(io)
}
fn selected(
    base: Option<&Block>,
    ours: Option<&Block>,
    theirs: Option<&Block>,
) -> Result<Option<Block>> {
    if let Some(base) = base {
        for side in [ours, theirs].into_iter().flatten() {
            if base.format != side.format || base.graph != side.graph {
                return Err(invalid("DataBook block format/graph identity changed"));
            }
        }
    }
    if ours == theirs || theirs == base {
        return Ok(ours.cloned());
    }
    if ours == base {
        return Ok(theirs.cloned());
    }
    let (Some(base), Some(ours), Some(theirs)) = (base, ours, theirs) else {
        return Err(invalid(
            "DataBook block add/add or delete/modify needs an explicit identity decision before merging",
        ));
    };
    if base.format != ours.format
        || base.format != theirs.format
        || base.graph != ours.graph
        || base.graph != theirs.graph
    {
        return Err(invalid("DataBook block format/graph identity changed"));
    }
    let mut block = ours.clone();
    block.wrapper = super::shapes(&base.wrapper, &ours.wrapper, &theirs.wrapper)?;
    if block.format.is_none() {
        block.payload = super::shapes(&base.payload, &ours.payload, &theirs.payload)?;
    }
    Ok(Some(block))
}

impl Plan {
    pub fn new(sources: &[String; 3], document: &str, commits: [String; 3]) -> Result<Self> {
        let inputs = sources
            .each_ref()
            .map(|source| crate::share::sha256(source.as_bytes()));
        let [base, ours, theirs] = sources;
        let changed = base != ours || base != theirs;
        let documents = [
            Document::parse(base)?,
            Document::parse(ours)?,
            Document::parse(theirs)?,
        ];
        if changed && documents.iter().any(|document| document.signed) {
            return Err(invalid(
                "DataBook document signatures cannot attest merged bytes; resolve signing in the branches",
            ));
        }
        if documents.iter().any(|doc| {
            doc.newline != documents[0].newline || doc.derived_id != documents[0].derived_id
        }) || (!documents[0].derived_id
            && documents
                .iter()
                .any(|doc| doc.identity != documents[0].identity))
        {
            return Err(invalid(
                "incompatible DataBook document identity/newline profile",
            ));
        }
        let envelope = super::shapes(
            &documents[0].envelope,
            &documents[1].envelope,
            &documents[2].envelope,
        )?;
        let ids: BTreeSet<_> = documents
            .iter()
            .flat_map(|doc| doc.blocks.keys().cloned())
            .collect();
        let mut blocks = BTreeMap::new();
        for id in ids {
            if let Some(block) = selected(
                documents[0].blocks.get(&id),
                documents[1].blocks.get(&id),
                documents[2].blocks.get(&id),
            )? {
                blocks.insert(id, block);
            }
        }
        let shapes = blocks
            .values()
            .filter(|block| block.format.is_none())
            .map(|block| block.payload.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let mut owners = BTreeMap::new();
        let mut all_graphs = BTreeSet::new();
        let mut snapshots = Vec::new();
        for doc in &documents {
            let mut graphs: BTreeMap<String, Graph> = BTreeMap::new();
            for block in doc.blocks.values() {
                for quad in &block.quads {
                    let key = slot(quad);
                    if owners.get(&key).is_some_and(|owner| owner != &block.id) {
                        return Err(invalid(
                            "DataBook slot ownership is ambiguous across blocks/snapshots",
                        ));
                    }
                    owners.insert(key, block.id.clone());
                    let graph = quad.graph_name.to_string();
                    all_graphs.insert(graph.clone());
                    graphs.entry(graph).or_default().insert(Triple::new(
                        quad.subject.clone(),
                        quad.predicate.clone(),
                        quad.object.clone(),
                    ));
                }
            }
            snapshots.push(graphs);
        }
        let empty = Graph::new();
        let mut graphs = BTreeMap::new();
        let mut groups = Vec::new();
        for graph in all_graphs {
            let (merged, decisions) = super::graph::prepare(
                snapshots[0].get(&graph).unwrap_or(&empty),
                snapshots[1].get(&graph).unwrap_or(&empty),
                snapshots[2].get(&graph).unwrap_or(&empty),
                &shapes,
                inputs.clone(),
            )?;
            graphs.insert(graph.clone(), merged);
            groups.push(Group { graph, decisions });
        }
        let file = File {
            schema: SCHEMA.into(),
            document: document.into(),
            commits,
            inputs,
            blocks: blocks
                .values()
                .map(|block| {
                    (
                        block.id.clone(),
                        block.graph.clone(),
                        format!("{:?}", block.format),
                    )
                })
                .collect(),
            shapes_hash: crate::share::sha256(shapes.as_bytes()),
            groups,
        };
        Ok(Self {
            documents,
            envelope,
            blocks,
            graphs,
            owners,
            shapes,
            file,
        })
    }
    pub fn apply(&mut self, recorded: Option<&File>) -> Result<(String, bool)> {
        if let Some(recorded) = recorded {
            let mut expected = self.file.clone();
            for (expected, recorded) in expected.groups.iter_mut().zip(&recorded.groups) {
                expected.decisions.resolutions = recorded.decisions.resolutions.clone();
                expected.decisions.resolved_by = recorded.decisions.resolved_by.clone();
            }
            if expected != *recorded {
                return Err(invalid("stale or altered DataBook merge decisions"));
            }
        }
        let mut ready = true;
        let mut quads = Vec::new();
        let mut payloads: BTreeMap<String, Vec<Quad>> = BTreeMap::new();
        for (index, group) in self.file.groups.iter_mut().enumerate() {
            let initial = text(&self.graphs[&group.graph]);
            let recorded = recorded.map(|file| &file.groups[index].decisions);
            let (graph, complete) = super::graph::apply(&initial, &mut group.decisions, recorded)?;
            ready &= complete;
            if complete && !self.shapes.trim().is_empty() {
                #[cfg(feature = "shacl")]
                if !crate::shacl::Validator::from_turtle(&self.shapes)?
                    .validate(text(&graph).as_bytes())?
                    .conforms
                {
                    return Err(invalid("merged DataBook graph fails SHACL"));
                }
                #[cfg(not(feature = "shacl"))]
                return Err(invalid("DataBook shape validation needs the shacl feature"));
            }
            for triple in graph {
                let quad = Quad::new(
                    triple.subject,
                    triple.predicate,
                    triple.object,
                    GraphName::NamedNode(graph_iri(&group.graph)?),
                );
                let owner = if let Some(owner) = self.owners.get(&slot(&quad)) {
                    owner.clone()
                } else {
                    // A reviewer-accepted alias assertion belongs with its uniquely owned subject.
                    let owners: BTreeSet<_> = self
                        .owners
                        .iter()
                        .filter(|((g, subject, _), _)| {
                            g == &group.graph && subject == &quad.subject.to_string()
                        })
                        .map(|(_, owner)| owner.clone())
                        .collect();
                    if owners.len() != 1 {
                        return Err(invalid("new DataBook assertion has no unique block owner"));
                    }
                    owners.into_iter().next().unwrap()
                };
                if !self.blocks.contains_key(&owner) {
                    return Err(invalid("merged DataBook graph refers to a removed block"));
                }
                payloads.entry(owner).or_default().push(quad.clone());
                quads.push(quad);
            }
        }
        let mut output_blocks = BTreeMap::new();
        for (id, block) in &self.blocks {
            let payload = if block.format.is_some() {
                let rows = payloads.get(id).map_or(&[][..], Vec::as_slice);
                // Reuse original bytes when the chosen graph is unchanged.
                let set = |rows: &[Quad]| {
                    rows.iter()
                        .map(ToString::to_string)
                        .collect::<BTreeSet<_>>()
                };
                self.documents
                    .iter()
                    .filter_map(|doc| doc.blocks.get(id))
                    .find(|old| set(&old.quads) == set(rows))
                    .map_or_else(|| block.serialize(rows), |old| Ok(old.payload.clone()))?
            } else {
                block.payload.clone()
            };
            output_blocks.insert(
                id.clone(),
                block.finish(&block.wrapper, &payload, self.documents[0].newline)?,
            );
        }
        let output = Document::finish(
            &self.envelope,
            &output_blocks,
            &quads,
            self.documents[0].derived_id,
        )?;
        Ok((output, ready))
    }
}

impl File {
    pub fn view(&self) -> crate::merge_reconcile::MergeView {
        use crate::merge_reconcile::{ReconcileFile, Surface};
        let mut view = ReconcileFile::Git {
            decisions: Decisions {
                schema: crate::merge_reconcile::GIT_DECISIONS_SCHEMA.into(),
                inputs: self.inputs.clone(),
                conflicts: vec![],
                kinds: vec![],
                aliases: vec![],
                resolutions: BTreeMap::new(),
                resolved_by: BTreeMap::new(),
            },
            dir: self.document.clone(),
            labels: BTreeMap::new(),
        }
        .view("", Surface::Cli);
        view.merge.kind = "databook";
        view.merge.title = format!(
            "DataBook '{}' (Git merge: MERGE_HEAD -> HEAD)",
            self.document
        );
        for (index, group) in self.groups.iter().enumerate() {
            let part = ReconcileFile::Git {
                decisions: group.decisions.clone(),
                dir: self.document.clone(),
                labels: BTreeMap::new(),
            }
            .view("", Surface::Cli);
            for mut card in part.conflicts {
                card.id = format!("graph:{index}/{}", card.id);
                card.rule.text = format!("{}: {}", group.graph, card.rule.text);
                view.conflicts.push(card);
            }
            for mut card in part.aliases {
                card.id = format!("graph:{index}/{}", card.id);
                view.aliases.push(card);
            }
            view.undecided += part.undecided;
        }
        view.next.clear();
        if let Some((id, choices)) = view
            .conflicts
            .iter()
            .find(|row| row.decision.is_none())
            .map(|row| (&row.id, &row.choices))
            .or_else(|| {
                view.aliases
                    .iter()
                    .find(|row| row.decision.is_none())
                    .map(|row| (&row.id, &row.choices))
            })
        {
            view.next.push(format!(
                "quipu merge resolve {}.decisions.json {id} --choose <{}> --by <you>",
                self.document,
                choices.join("|")
            ));
        }
        view.next.push(format!("git add {} {}.decisions.json && git commit (CI databook-check verifies immutable inputs)", self.document, self.document));
        view
    }
}
