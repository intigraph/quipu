//! Guarded portable workflow operations.
use serde_json::Value;

pub(super) fn defs() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": "quipu_reason",
            "description": "Run stored or inline Datalog rules to fixpoint and persist derivations. Omitted graph means ROOT; pass graph to scope premises. Derivations persist in that graph's reserved companion inferred graph, not alongside asserted facts.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "rules": {
                        "type": "string"
                    },
                    "prefix": {
                        "type": "string"
                    },
                    "graph": {
                        "type": "string"
                    },
                    "timestamp": {
                        "type": "string"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_update",
            "description": "Execute SPARQL 1.1 Update through the guarded update endpoint. Use explicit GRAPH clauses for named graphs; absent scope has standard SPARQL default-graph semantics. Actor/source are declared provenance, not authorization.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "update": {
                        "type": "string"
                    },
                    "actor": {
                        "type": "string"
                    },
                    "source": {
                        "type": "string"
                    }
                },
                "required": [
                    "update"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_events",
            "description": "Read a bounded event batch without committing a consumer cursor. since overrides consumer. types is a comma-separated event-type filter; group is an episode group, not a graph selector.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer"
                    },
                    "limit": {
                        "type": "integer"
                    },
                    "types": {
                        "type": "string"
                    },
                    "group": {
                        "type": "string"
                    },
                    "consumer": {
                        "type": "string"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_events_commit",
            "description": "Durably set a consumer cursor. Lower offsets explicitly rewind; delivery alone never commits.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "consumer_id": {
                        "type": "string"
                    },
                    "offset": {
                        "type": "integer",
                        "minimum": 0
                    }
                },
                "required": [
                    "consumer_id",
                    "offset"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_changes",
            "description": "Read whole transaction change pages. graph scopes an exact graph; use next_tx for the next page. No cursor write.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": {
                        "type": "integer"
                    },
                    "limit": {
                        "type": "integer"
                    },
                    "capture": {
                        "type": "string",
                        "enum": [
                            "new_values",
                            "old_and_new_values",
                            "new_row"
                        ]
                    },
                    "graph": {
                        "type": "string"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_subscriptions",
            "description": "Manage persistent push subscriptions (create/list/delete). Creates may deliver externally; list is still conservatively write-classified like REST.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "consumer_id": {
                        "type": "string"
                    },
                    "webhook_url": {
                        "type": "string"
                    },
                    "sparql_ask": {
                        "type": "string"
                    },
                    "action": {
                        "type": "string",
                        "enum": [
                            "create",
                            "list",
                            "delete"
                        ]
                    },
                    "types": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "mode": {
                        "type": "string",
                        "enum": [
                            "realtime",
                            "batch"
                        ]
                    },
                    "batch_size": {
                        "type": "integer"
                    },
                    "batch_window_s": {
                        "type": "integer"
                    }
                },
                "required": [],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_workflow_capabilities",
            "description": "Read whether the server enforces direct-workflow guards before using quipu_update. Reports capability only, never configuration or credentials.",
            "inputSchema": {
                "type": "object",
                "properties": {},
                "additionalProperties": false
            }
        }),
    ]
}
