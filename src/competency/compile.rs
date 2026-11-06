//! Deterministic RDF compiler; ordering in TOML does not affect the result.
use super::*;
use crate::share_queries::SharedQuery;
use crate::store::queries::{StoredParam, StoredQuery};

const HEADER: &str = "@prefix quipu: <https://quipu.dev/ontology/> .\n";
fn literal(text: &str) -> String {
    oxrdf::Literal::new_simple_literal(text).to_string()
}

fn declarations(
    out: &mut String,
    subject: &str,
    predicate: &str,
    values: &BTreeMap<String, String>,
) {
    for (name, kind) in values {
        out.push_str(&format!(
            "<{subject}> quipu:{predicate} [ quipu:name {} ; quipu:value {} ] .\n",
            literal(name),
            literal(kind)
        ));
    }
}
fn links(out: &mut String, subject: &str, predicate: &str, values: &[String]) {
    for value in values.iter().collect::<BTreeSet<_>>() {
        out.push_str(&format!("<{subject}> quipu:{predicate} <{value}> .\n"));
    }
}

impl Contract {
    /// Compile validated TOML to canonical RDF without evaluating it.
    ///
    /// Retired blocks are retained with their explicit retirement flag. This
    /// compiler adds contracts to `StoredQuery` RDF; optional parameters retain
    /// `required=false` with no invented execution default. A consumer must
    /// implement the competency contract before executing those definitions.
    ///
    /// # Errors
    /// The same validation refusals as [`Contract::lint`].
    pub fn compile(&self, registry: &[SharedQuery]) -> Result<Compiled> {
        self.lint(registry)?;
        let mut queries = String::from(HEADER);
        let mut commands = String::from(HEADER);
        let mut questions: Vec<_> = self.questions.iter().collect();
        questions.sort_by_key(|q| &q.id);
        for q in questions {
            let text = lint::query_text(q, registry)?;
            // Lint parsed the real template with typed probes; this form is
            // derived there as well, never by inspecting a leading keyword.
            let form = if q.returns_shape.is_some() && q.returns.is_empty() {
                "CONSTRUCT"
            } else if q.returns.is_empty() {
                "ASK"
            } else {
                "SELECT"
            };
            let shared = SharedQuery {
                query: StoredQuery {
                    name: q.id.clone(),
                    description: q.ask.clone(),
                    template: text,
                    dataset: None,
                    params: q
                        .inputs
                        .iter()
                        .map(|(name, kind)| StoredParam {
                            name: name.clone(),
                            kind: kind.trim_end_matches('?').to_owned(),
                            required: !kind.ends_with('?'),
                            default: None,
                            description: String::new(),
                        })
                        .collect(),
                },
                form,
                targets: Vec::new(),
            };
            queries.push_str(&crate::share_queries::to_turtle(&[shared]));
            let subject = format!("urn:quipu:query:{}", q.id);
            queries.push_str(&format!("<{subject}> quipu:retired {} .\n", q.retired));
            declarations(&mut queries, &subject, "returns", &q.returns);
            declarations(&mut queries, &subject, "example", &q.example);
            links(&mut queries, &subject, "justifies", &q.justifies);
            if let Some(shape) = &q.returns_shape {
                links(
                    &mut queries,
                    &subject,
                    "returnsShape",
                    std::slice::from_ref(shape),
                );
            }
            if let Some(expect) = &q.expect {
                queries.push_str(&format!("<{subject}> quipu:expect {} .\n", literal(expect)));
            }
            if let Some(reference) = &q.query_ref {
                queries.push_str(&format!(
                    "<{subject}> quipu:queryRef {} .\n",
                    literal(reference)
                ));
            }
        }
        let mut sorted: Vec<_> = self.commands.iter().collect();
        sorted.sort_by_key(|c| &c.id);
        for c in sorted {
            let subject = format!("urn:quipu:command:{}", c.id);
            commands.push_str(&format!("<{subject}> a quipu:Command ; quipu:commandName {} ; quipu:does {} ; quipu:template {} ; quipu:writeClass {} ; quipu:retired {} .\n", literal(&c.id), literal(&c.does), literal(&c.template), literal(&c.write_class), c.retired));
            declarations(&mut commands, &subject, "input", &c.inputs);
            declarations(&mut commands, &subject, "example", &c.example);
            links(&mut commands, &subject, "satisfies", &c.satisfies);
            for key in c.key.iter().collect::<BTreeSet<_>>() {
                commands.push_str(&format!("<{subject}> quipu:key {} .\n", literal(key)));
            }
        }
        Ok(Compiled { queries, commands })
    }
}
