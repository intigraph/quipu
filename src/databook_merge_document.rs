//! Lossless document spans; import's intentionally lossy view is never serialized.
use super::super::{invalid, io};
use crate::databook::{self, BlockKind};
use crate::error::Result;
use crate::schema::ROOT_GRAPH_IRI;
use oxrdf::{GraphName, NamedOrBlankNode, Quad, Term};
use oxrdfio::{RdfFormat, RdfParser, RdfSerializer};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(super) const PAYLOAD: &str = "@@QUIPU_PAYLOAD@@";
const HASH: &str = "@@QUIPU_HASH@@";
const ID: &str = "@@QUIPU_BOOK_ID@@";
const COUNT: &str = "@@QUIPU_TRIPLES@@";
const SUBJECTS: &str = "@@QUIPU_SUBJECTS@@";

pub(super) fn hash(text: &str) -> String {
    hex::encode(Sha256::digest(text.trim_end_matches('\n').as_bytes()))
}

#[derive(Clone, PartialEq)]
pub(super) struct Block {
    pub id: String,
    pub graph: String,
    pub format: Option<RdfFormat>, // None is a SHACL fence.
    pub wrapper: String,
    pub payload: String,
    pub raw: String,
    pub quads: Vec<Quad>,
    pub signature: bool,
}
impl Block {
    pub fn token(&self) -> String {
        format!("@@QUIPU_BLOCK_{}@@", hex::encode(&self.id))
    }
    pub fn finish(&self, wrapper: &str, payload: &str, newline: &str) -> Result<String> {
        if self.signature && payload.trim_end() != self.payload.trim_end() {
            return Err(invalid(
                "DataBook payload signature cannot be retained after rewriting",
            ));
        }
        Ok(wrapper
            .replace(HASH, &hash(payload))
            .replace(PAYLOAD, &payload.replace('\n', newline)))
    }
    pub fn serialize(&self, quads: &[Quad]) -> Result<String> {
        let format = self.format.ok_or_else(|| invalid("not an RDF block"))?;
        let mut quads = quads.to_vec();
        quads.sort_by_key(ToString::to_string);
        let mut serializer = RdfSerializer::from_format(format).for_writer(Vec::new());
        for mut quad in quads {
            if !matches!(format, RdfFormat::TriG | RdfFormat::NQuads) {
                if quad.graph_name.to_string() != format!("<{}>", self.graph) {
                    return Err(invalid("merged DataBook block changed graph identity"));
                }
                quad.graph_name = GraphName::DefaultGraph;
            }
            serializer.serialize_quad(&quad).map_err(io)?;
        }
        String::from_utf8(serializer.finish().map_err(io)?).map_err(io)
    }
}

pub(super) struct Document {
    pub envelope: String,
    pub blocks: BTreeMap<String, Block>,
    pub newline: &'static str,
    pub derived_id: bool,
    pub identity: String,
    pub signed: bool,
}

fn count_field(text: &str, key: &str, replacement: &str) -> Result<String> {
    let (_, body_start, _) = databook::frontmatter(&text.lines().collect::<Vec<_>>())?;
    let mut in_graph = false;
    let mut count = 0;
    let mut out = String::new();
    for (index, line) in text.split_inclusive('\n').enumerate() {
        if index >= body_start {
            out.push_str(line);
            continue;
        }
        if line.trim_end() == "graph:" {
            in_graph = true;
        } else if !line.starts_with(' ') && !line.trim().is_empty() {
            in_graph = false;
        }
        if in_graph && line.starts_with(&format!("  {key}: ")) {
            count += 1;
            let end = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            out.push_str(&format!("  {key}: {replacement}{end}"));
        } else {
            out.push_str(line);
        }
    }
    if count > 1 {
        return Err(invalid("ambiguous DataBook graph count field"));
    }
    Ok(out)
}

impl Document {
    pub fn parse(source: &str) -> Result<Self> {
        if source.contains("@@QUIPU_") || source.contains('\0') {
            return Err(invalid("DataBook contains reserved merge tokens"));
        }
        let newline = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        if source.replace("\r\n", "").contains('\r')
            || (newline == "\r\n" && source.replace("\r\n", "").contains('\n'))
        {
            return Err(invalid(
                "DataBook merge requires one unambiguous newline convention",
            ));
        }
        let book = databook::parse(source)?;
        let (yaml, _, _) = databook::frontmatter(&source.lines().collect::<Vec<_>>())?;
        let signed = yaml
            .lines()
            .filter_map(|line| {
                line.split_once(':')
                    .map(|(key, _)| key.to_ascii_lowercase())
            })
            .any(|key| key.contains("signature") || key.contains("attestation"));
        let lines: Vec<_> = source.split_inclusive('\n').collect();
        let mut offsets = vec![0];
        for line in &lines {
            offsets.push(offsets.last().unwrap() + line.len());
        }
        let mut blocks = BTreeMap::new();
        let mut spans = Vec::new();
        for block in &book.blocks {
            let format = match block.kind() {
                BlockKind::Rdf(format) => Some(format),
                BlockKind::Shapes => None,
                BlockKind::Opaque | BlockKind::Unsupported => {
                    return Err(invalid(
                        "unsupported DataBook data/reference/encryption profile",
                    ));
                }
                _ => continue, // Queries, updates and display fences are inert envelope bytes.
            };
            let id = block
                .id()
                .filter(|id| !id.is_empty())
                .ok_or_else(|| {
                    invalid(
                        "DataBook merge needs an explicit unique databook:id per data/shape block",
                    )
                })?
                .to_string();
            let opening = block.line - 1;
            let clean = |index: usize| lines[index].trim_end_matches(['\r', '\n']);
            let (ticks, _) = databook::fence_open(clean(opening))
                .ok_or_else(|| invalid("DataBook fence span mismatch"))?;
            let mut start = opening;
            if start > 0 && clean(start - 1).trim().is_empty() {
                start -= 1;
            }
            while start > 0
                && (databook::annotation(clean(start - 1)).is_some()
                    || databook::directive(clean(start - 1)).is_some())
            {
                start -= 1;
            }
            let mut payload_start = opening + 1;
            while payload_start < lines.len()
                && databook::annotation(clean(payload_start)).is_some()
            {
                payload_start += 1;
            }
            let close = (payload_start..lines.len())
                .find(|index| databook::fence_close(clean(*index), ticks))
                .ok_or_else(|| invalid("DataBook fence has no closing span"))?;
            if (payload_start..close).any(|index| databook::annotation(clean(index)).is_some()) {
                return Err(invalid(
                    "move inline DataBook annotations before the payload before merging",
                ));
            }
            let payload = source[offsets[payload_start]..offsets[close]].replace("\r\n", "\n");
            let mut wrapper = String::new();
            let mut keys = BTreeSet::new();
            for (index, &line) in lines.iter().enumerate().take(payload_start).skip(start) {
                if let Some((key, value)) = databook::annotation(clean(index)) {
                    if !keys.insert(key.clone()) {
                        return Err(invalid("ambiguous duplicate DataBook block annotation"));
                    }
                    if key == "quipu:sha256" {
                        if value != hash(&block.payload) {
                            return Err(invalid("DataBook declared payload hash mismatch"));
                        }
                        let at = line
                            .rfind(&value)
                            .ok_or_else(|| invalid("DataBook hash annotation span mismatch"))?;
                        wrapper.push_str(&line[..at]);
                        wrapper.push_str(HASH);
                        wrapper.push_str(&line[at + value.len()..]);
                        continue;
                    }
                }
                wrapper.push_str(line);
            }
            wrapper.push_str(PAYLOAD);
            wrapper.push_str(lines[close]);
            let graph = block
                .meta
                .get("graph")
                .or(book.header.named_graph.as_ref())
                .cloned()
                .unwrap_or_else(|| ROOT_GRAPH_IRI.into());
            oxrdf::NamedNode::new(&graph).map_err(io)?;
            let mut quads = Vec::new();
            if let Some(format) = format {
                for quad in RdfParser::from_format(format).for_reader(block.payload.as_bytes()) {
                    let mut quad = quad.map_err(io)?;
                    if matches!(quad.subject, NamedOrBlankNode::BlankNode(_))
                        || matches!(quad.object, Term::BlankNode(_))
                        || matches!(quad.graph_name, GraphName::BlankNode(_))
                    {
                        return Err(invalid(
                            "DataBook Git merge needs explicit cross-snapshot blank-node identity; skolemize first",
                        ));
                    }
                    if quad.graph_name == GraphName::DefaultGraph {
                        quad.graph_name = oxrdf::NamedNode::new(&graph).map_err(io)?.into();
                    }
                    quads.push(quad);
                }
            }
            let raw = source[offsets[start]..offsets[close + 1]].to_string();
            let block = Block {
                id: id.clone(),
                graph,
                format,
                wrapper,
                payload,
                raw,
                quads,
                signature: block
                    .meta
                    .keys()
                    .any(|key| key.contains("signature") || key.contains("attestation")),
            };
            spans.push((offsets[start], offsets[close + 1], block.token()));
            if blocks.insert(id, block).is_some() {
                return Err(invalid("duplicate DataBook block identity"));
            }
        }
        if blocks.values().all(|block| block.format.is_none()) {
            return Err(invalid("DataBook merge needs an RDF data block"));
        }
        let mut envelope = source.to_string();
        for (start, end, token) in spans.iter().rev() {
            envelope.replace_range(*start..*end, token);
        }
        let derived_id = book.header.id.starts_with("urn:quipu:databook:sha256:");
        if derived_id {
            let data: Vec<_> = blocks
                .values()
                .filter(|block| block.format.is_some())
                .collect();
            if data.len() != 1
                || book.header.id != format!("urn:quipu:databook:sha256:{}", hash(&data[0].payload))
                || envelope.matches(&book.header.id).count() != 1
            {
                return Err(invalid("unproved DataBook derived document identity"));
            }
            envelope = envelope.replace(&book.header.id, ID);
        }
        let count: BTreeSet<_> = blocks
            .values()
            .flat_map(|block| &block.quads)
            .map(ToString::to_string)
            .collect();
        if book
            .header
            .triple_count
            .is_some_and(|n| n != count.len() as u64)
        {
            return Err(invalid("DataBook triple_count does not match data"));
        }
        let old = envelope.clone();
        envelope = count_field(&envelope, "triple_count", COUNT)?;
        if book.header.triple_count.is_some() && envelope == old {
            return Err(invalid(
                "DataBook count field is not a supported lossless scalar",
            ));
        }
        envelope = count_field(&envelope, "subjects", SUBJECTS)?;
        Ok(Self {
            envelope,
            blocks,
            newline,
            derived_id,
            identity: book.header.id,
            signed,
        })
    }
    pub fn finish(
        envelope: &str,
        blocks: &BTreeMap<String, String>,
        quads: &[Quad],
        derived: bool,
    ) -> Result<String> {
        let mut output = envelope.to_string();
        for (id, bytes) in blocks {
            output = output.replace(&format!("@@QUIPU_BLOCK_{}@@", hex::encode(id)), bytes);
        }
        let count: BTreeSet<_> = quads.iter().map(ToString::to_string).collect();
        let subjects: BTreeSet<_> = quads.iter().map(|quad| quad.subject.to_string()).collect();
        output = output
            .replace(COUNT, &count.len().to_string())
            .replace(SUBJECTS, &subjects.len().to_string());
        if derived {
            let parsed = databook::parse(&output.replace(ID, "urn:pending"))?;
            let data: Vec<_> = parsed
                .blocks
                .iter()
                .filter(|block| matches!(block.kind(), BlockKind::Rdf(_)))
                .collect();
            if data.len() != 1 {
                return Err(invalid(
                    "merged derived DataBook identity requires one data block",
                ));
            }
            output = output.replace(
                ID,
                &format!("urn:quipu:databook:sha256:{}", hash(&data[0].payload)),
            );
        }
        if output.contains("@@QUIPU_") {
            return Err(invalid("DataBook envelope/block membership mismatch"));
        }
        Self::parse(&output)?;
        Ok(output)
    }
}
