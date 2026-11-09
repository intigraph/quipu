//! Store-free pendant merging. Git supplies the base; the store merge supplies the operator.
use crate::error::{Error, Result};
use crate::git_merge_alias::{AliasProposal, propose};
pub use crate::git_merge_repo::{
    check, check_history, driver, merge, merge_refs, merge_with_pending, reconcile, resolve,
    resolve_by,
};
use crate::share::{ShareManifest, canonicalize_ntriples, manifest_bytes, sha256};
use crate::share_merge::{DecisionRecord, Graph, parse_graph};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[path = "git_merge_core.rs"]
mod core;
#[path = "databook_merge.rs"]
pub mod databook;
#[path = "git_merge_graph.rs"]
mod graph;

#[derive(Clone)]
pub(crate) struct Pack {
    pub graph: String,
    pub shapes: String,
    pub manifest: ShareManifest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decisions {
    pub schema: String,
    pub inputs: [String; 3],
    pub conflicts: Vec<DecisionRecord>,
    /// v2 report kinds aligned with rows; v1 files omit them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<crate::share_merge_decisions_view::ConflictKind>,
    pub aliases: Vec<AliasProposal>,
    #[serde(default)]
    pub resolutions: BTreeMap<String, String>,
    /// Who recorded each resolution and when (`"<who> at <time>"`), as the
    /// resolving tool claims it (aegis-k7l). Display only: the merge result is
    /// fixed by `resolutions` alone.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub resolved_by: BTreeMap<String, String>,
}

/// The values a conflict resolves to under `choice`, or why it cannot.
pub(crate) fn resolved_values(c: &DecisionRecord, choice: &str) -> Result<Vec<String>> {
    core::resolve(c, choice)
}

pub(crate) struct Merged {
    pub pack: Pack,
    pub decisions: Decisions,
}

pub(crate) fn invalid(msg: impl Into<String>) -> Error {
    Error::InvalidValue(msg.into())
}
pub(crate) fn io(e: impl std::fmt::Display) -> Error {
    Error::Store(e.to_string())
}
/// Map a failure to start `git` into an actionable error: every pendant Git
/// command shells out to the `git` executable found on PATH.
pub(crate) fn spawn_git(e: &std::io::Error) -> Error {
    if e.kind() == std::io::ErrorKind::NotFound {
        invalid("`git` executable not found on PATH; pendant Git commands require git")
    } else {
        io(format!("failed to run git: {e}"))
    }
}

impl Pack {
    pub(crate) fn verify(&self) -> Result<()> {
        let m = &self.manifest;
        if m.schema != "https://github.com/scbrown/quipu/share-manifest/v1"
            || m.files.graph != "export.nt"
            || m.files.shapes != "shapes.ttl"
            || m.share_id != sha256(&manifest_bytes(m, false)?)
            || m.graph_hash != sha256(self.graph.as_bytes())
            || m.shapes_hash != sha256(self.shapes.as_bytes())
        {
            return Err(invalid("pendant envelope/hash mismatch"));
        }
        parse_graph(&self.graph, "pendant")?;
        if self.shapes.trim().is_empty() {
            return Err(invalid("pendant merge requires non-empty shapes"));
        }
        Ok(())
    }

    fn rehash(&mut self) -> Result<()> {
        self.graph =
            String::from_utf8(canonicalize_ntriples(self.graph.as_bytes())?).map_err(io)?;
        self.manifest.graph_hash = sha256(self.graph.as_bytes());
        self.manifest.shapes_hash = sha256(self.shapes.as_bytes());
        self.manifest.canonicalization = Some("RDFC-1.0".into());
        self.manifest.attestation = None; // Old producer signatures do not attest a new merge.
        self.manifest.files.turtle_view = None;
        self.manifest.share_id = sha256(&manifest_bytes(&self.manifest, false)?);
        Ok(())
    }
}

fn render(g: &Graph) -> String {
    let mut lines: Vec<_> = g.iter().map(|t| format!("{t} .\n")).collect();
    lines.sort();
    lines.concat()
}

fn shapes(base: &str, ours: &str, theirs: &str) -> Result<String> {
    if ours == theirs || theirs == base {
        return Ok(ours.into());
    }
    if ours == base {
        return Ok(theirs.into());
    }
    let dir = tempfile::tempdir().map_err(io)?;
    for (name, bytes) in [("base", base), ("ours", ours), ("theirs", theirs)] {
        std::fs::write(dir.path().join(name), bytes).map_err(io)?;
    }
    let out = std::process::Command::new("git")
        .args(["merge-file", "-p", "--diff3"])
        .arg(dir.path().join("ours"))
        .arg(dir.path().join("base"))
        .arg(dir.path().join("theirs"))
        .output()
        .map_err(|e| spawn_git(&e))?;
    if !out.status.success() {
        return Err(invalid(
            "overlapping shapes edits: resolve shapes in the branches before merging",
        ));
    }
    String::from_utf8(out.stdout).map_err(io)
}

pub(crate) fn validate(pack: &Pack) -> Result<()> {
    pack.verify()?;
    #[cfg(feature = "shacl")]
    {
        let result =
            crate::shacl::Validator::from_turtle(&pack.shapes)?.validate(pack.graph.as_bytes())?;
        if !result.conforms {
            return Err(invalid(format!(
                "merged pendant fails SHACL: {} violations",
                result.violations
            )));
        }
        Ok(())
    }
    #[cfg(not(feature = "shacl"))]
    Err(invalid(
        "pendant merge/check requires a build with the shacl feature",
    ))
}

pub(crate) fn plan(base: &Pack, ours: &Pack, theirs: &Pack) -> Result<Merged> {
    for p in [base, ours, theirs] {
        p.verify()?;
    }
    // Refuse envelope changes that cannot be reconciled as derived payload metadata.
    if ours.manifest.store_id != theirs.manifest.store_id
        || ours.manifest.scope != theirs.manifest.scope
        || ours.manifest.destination != theirs.manifest.destination
        || ours.manifest.pack_dir != theirs.manifest.pack_dir
    {
        return Err(invalid(
            "incompatible pendant store/scope/destination/layout",
        ));
    }
    let shapes = shapes(&base.shapes, &ours.shapes, &theirs.shapes)?;
    let (b, o, t) = (
        parse_graph(&base.graph, "base")?,
        parse_graph(&ours.graph, "ours")?,
        parse_graph(&theirs.graph, "theirs")?,
    );
    // Canonical blank labels are not stable across changing snapshots. Refuse until an
    // explicit cross-snapshot identity contract exists, rather than conflating nodes.
    if b.iter().chain(&o).chain(&t).any(|t| {
        matches!(t.subject, oxrdf::NamedOrBlankNode::BlankNode(_))
            || matches!(t.object, oxrdf::Term::BlankNode(_))
    }) {
        return Err(invalid(
            "Git pendant merge requires IRI subjects/objects; skolemize blank nodes first",
        ));
    }
    let (graph, decisions) = graph::prepare(
        &b,
        &o,
        &t,
        &shapes,
        [
            base.manifest.share_id.clone(),
            ours.manifest.share_id.clone(),
            theirs.manifest.share_id.clone(),
        ],
    )?;
    let mut pack = Pack {
        graph: render(&graph),
        shapes,
        manifest: ours.manifest.clone(),
    };
    pack.manifest.parent_share = Some(ours.manifest.share_id.clone());
    pack.manifest.merge_parents = vec![
        ours.manifest.share_id.clone(),
        theirs.manifest.share_id.clone(),
    ];
    pack.rehash()?;
    Ok(Merged { pack, decisions })
}

impl Merged {
    pub(crate) fn apply(&mut self, recorded: Option<&Decisions>) -> Result<bool> {
        let (graph, ready) = graph::apply(&self.pack.graph, &mut self.decisions, recorded)?;
        self.pack.graph = render(&graph);
        self.pack.rehash()?;
        if ready {
            validate(&self.pack)?;
        }
        Ok(ready)
    }
}
