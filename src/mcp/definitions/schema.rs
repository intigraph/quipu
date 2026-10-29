//! Schema proposals and the ask surface.
//!
//! Split out of `definitions.rs` under the file-size ratchet (aegis-gf3j7). The
//! blocks are MOVED VERBATIM and their order is preserved, so `tool_definitions()`
//! returns exactly the Vec it returned before — the split is provable, not argued.

use serde_json::Value as JsonValue;

pub(super) fn defs() -> Vec<JsonValue> {
    vec![
        serde_json::json!({
            "name": "quipu_ask",
            "description": "Run a curated, parameterized named query by name instead of hand-writing SPARQL. Call with no 'name' (or name='list') to discover the self-describing catalog of available queries and their parameters (e.g. service_deps, references_to, entity_facts, entities_of_type, labeled_like).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "graph": {"type": "string", "description": "Named graph override for the stored query."},
                    "name": { "type": "string", "description": "Named query to run; omit (or 'list') to list the catalog." },
                    "params": { "type": "object", "description": "Parameter map for the named query (see catalog for names/types)." }
                }
            }
        }),
        serde_json::json!({
            "name": "quipu_propose_schema_change",
            "description": "Submit a schema evolution proposal (new shape, class, property, or ontology change). Proposals require explicit acceptance before taking effect.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["shape", "ontology", "class", "property"], "description": "Kind of schema change" },
                    "target": { "type": "string", "description": "Shape name, class IRI, or property IRI being changed" },
                    "diff": { "type": "string", "description": "Turtle fragment or JSON patch describing the change" },
                    "rationale": { "type": "string", "description": "Why this change is needed" },
                    "proposer": { "type": "string", "description": "Identity of the proposing agent" },
                    "trigger_ref": { "type": "string", "description": "Validation failure ref or bead id that triggered this proposal" },
                    "migration": {
                        "type": "array",
                        "description": "Optional data-migration program run as ONE transaction (source migration:<id>) when the proposal is accepted. Each item has an 'op': rename_predicate {from,to}; retype_class {from,to}; cast_datatype {predicate,datatype}; split_property {from, into:[{predicate,expression}]} (expressions over ?o); merge_property {from:[..], to, expression?} (expression over ?v0,?v1,..); sparql {update} (DELETE/INSERT WHERE, INSERT DATA or DELETE DATA on the ROOT graph). Old facts are superseded, not rewritten.",
                        "items": { "type": "object" }
                    },
                    "timestamp": { "type": "string", "description": "ISO-8601 timestamp" }
                },
                "required": ["kind", "target", "diff", "proposer"]
            }
        }),
        serde_json::json!({
            "name": "quipu_list_proposals",
            "description": "List schema evolution proposals, optionally filtered by status (pending, accepted, rejected)",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": { "type": "string", "enum": ["pending", "accepted", "rejected"], "description": "Filter by proposal status (default: all)" }
                }
            }
        }),
        serde_json::json!({
            "name": "quipu_accept_proposal",
            "description": "Accept a pending schema proposal. For shape proposals, validates the Turtle before writing to the shapes table. A proposal carrying a migration runs it in the same transaction; a refused migration leaves the proposal pending and nothing written. With dry_run=true nothing is accepted: the report lists existing facts the new shapes would refuse, what the migration would change, and whether acceptance would go through.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Proposal ID to accept" },
                    "decided_by": { "type": "string", "description": "Identity of the approver (required; there is no default)" },
                    "note": { "type": "string", "description": "Optional acceptance note" },
                    "dry_run": { "type": "boolean", "description": "Report what acceptance would do without accepting (default false)" },
                    "limit": { "type": "integer", "description": "Dry run: maximum sample facts and violations per list (default 50, max 1000)" },
                    "timestamp": { "type": "string", "description": "ISO-8601 timestamp" }
                },
                "required": ["id", "decided_by"]
            }
        }),
        serde_json::json!({
            "name": "quipu_reject_proposal",
            "description": "Reject a pending schema proposal with a reason",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "id": { "type": "integer", "description": "Proposal ID to reject" },
                    "decided_by": { "type": "string", "description": "Identity of the rejector (required; there is no default)" },
                    "note": { "type": "string", "description": "Reason for rejection" },
                    "timestamp": { "type": "string", "description": "ISO-8601 timestamp" }
                },
                "required": ["id", "decided_by", "note"]
            }
        }),
    ]
}
