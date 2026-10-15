//! Guarded portable discovery operations.
use serde_json::Value;

pub(super) fn defs() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": "quipu_entity",
            "description": "Dereference an exact IRI as JSON-LD. IRIs are encoded as query values, including slashes and fragments; expanded includes label expansion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "iri": {
                        "type": "string"
                    },
                    "expanded": {
                        "type": "boolean"
                    }
                },
                "required": [
                    "iri"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_entity_history",
            "description": "Read the existing entity-history endpoint for an exact IRI. Its current MAIN-history semantics do not certify composed/named-graph audit completeness.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "iri": {
                        "type": "string"
                    }
                },
                "required": [
                    "iri"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_transactions",
            "description": "Read bounded transaction metadata after since; does not change a cursor.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer"
                    },
                    "limit": {
                        "type": "integer"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_embed_backfill",
            "description": "Backfill missing ROOT entity embeddings using the configured provider. May consume external embedding resources and persist vectors.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_embed_backfill_graph",
            "description": "Backfill a bounded number of named-graph entity embeddings; does not rewrite ROOT entity text.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "graph": {
                        "type": "string"
                    },
                    "max_entities": {
                        "type": "integer"
                    }
                },
                "required": [
                    "graph"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_search_query",
            "description": "Fuse ranked search seeds with a SPARQL query. Existing mode-specific validation and query budgets apply.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string"
                    },
                    "query": {
                        "type": "string"
                    },
                    "seed_variable": {
                        "type": "string"
                    },
                    "sparql": {
                        "type": "string"
                    },
                    "valid_at": {
                        "type": "string"
                    },
                    "entity_type": {
                        "type": "string"
                    },
                    "embedding": {
                        "type": "array",
                        "items": {
                            "type": "number"
                        }
                    },
                    "seed_limit": {
                        "type": "integer"
                    },
                    "query_options": {
                        "type": "object"
                    },
                    "group_ids": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_fragments",
            "description": "Read a bounded triple-pattern fragment. page and pageSize select a page; absent terms are wildcards.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "subject": {
                        "type": "string"
                    },
                    "predicate": {
                        "type": "string"
                    },
                    "object": {
                        "type": "string"
                    },
                    "page": {
                        "type": "integer"
                    },
                    "pageSize": {
                        "type": "integer"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_reconcile",
            "description": "Get the reconciliation service manifest or resolve keyed query objects without merging entities.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "queries": {
                        "type": "object"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_spotlight",
            "description": "Annotate text with known entities, read-only. A busy cold cache may return no annotations; that is not an absence proof.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string"
                    },
                    "confidence": {
                        "type": "number"
                    }
                },
                "required": [
                    "text"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_stats",
            "description": "Read server graph and transaction statistics. This describes the serving store, not a client-local database.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_health",
            "description": "Read server health as a positive liveness control; healthy does not prove complete graph coverage.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_version",
            "description": "Read served version, source commit and dirty-build identity through MCP without a REST fallback.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
    ]
}
