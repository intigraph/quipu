//! Guarded portable publication operations.
use serde_json::Value;

pub(super) fn defs() -> Vec<Value> {
    vec![
        serde_json::json!({
            "name": "quipu_share",
            "description": "Produce a bounded portable share payload, including canonical manifest and member bytes. Read-only; no server-local output path. Pass the exact scope. Internal-only export exemptions are not accepted over this transport.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "scope": {
                        "type": "object",
                        "properties": {
                            "kind": {
                                "type": "string",
                                "enum": [
                                    "root",
                                    "graph",
                                    "group",
                                    "construct"
                                ]
                            },
                            "value": {
                                "type": [
                                    "string",
                                    "null"
                                ]
                            }
                        },
                        "required": [
                            "kind"
                        ],
                        "additionalProperties": false
                    },
                    "shapes": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "no_shapes": {
                        "type": "boolean"
                    },
                    "queries": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "parent_share": {
                        "type": "string"
                    },
                    "turtle_view": {
                        "type": "boolean"
                    },
                    "max_bytes": {
                        "type": "integer"
                    }
                },
                "required": [
                    "scope"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_import",
            "description": "Stage a portable share with hash, vocabulary and attestation gates. Copy member contents from quipu_share; this is not a server file path. Exact-name merges default off; review before promotion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "manifest": {
                        "type": "object"
                    },
                    "export_ntriples": {
                        "type": "string"
                    },
                    "shapes_turtle": {
                        "type": "string"
                    },
                    "queries_turtle": {
                        "type": "string"
                    },
                    "query_namespace": {
                        "type": "string"
                    },
                    "replace_queries": {
                        "type": "boolean"
                    },
                    "source": {
                        "type": "string"
                    },
                    "actor": {
                        "type": "string"
                    },
                    "accept_exact": {
                        "type": "boolean"
                    },
                    "attestation": {
                        "type": "object"
                    }
                },
                "required": [
                    "manifest",
                    "export_ntriples",
                    "shapes_turtle",
                    "source"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_import_promote",
            "description": "Promote a staged share only when existing promotion gates permit it. Does not bypass quarantine or resolution review.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "share_id": {
                        "type": "string"
                    },
                    "actor": {
                        "type": "string"
                    }
                },
                "required": [
                    "share_id"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_acquire",
            "description": "Acquire complete bounded registered named graphs from an expected MAIN store lineage. Unknown graphs, unsupported attachments/pins and overflow refuse.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_store_id": {
                        "type": "string"
                    },
                    "graphs": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "tx": {
                        "type": "integer"
                    },
                    "max_bytes": {
                        "type": "integer"
                    }
                },
                "required": [
                    "source_store_id",
                    "graphs"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_acquire_composed",
            "description": "Acquire qualified MAIN graphs with a fenced attachment census. expected_generation is required with an explicit main tx; provider clocks are never a global transaction.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source_store_id": {
                        "type": "string"
                    },
                    "graphs": {
                        "type": "array",
                        "items": {
                            "type": "string"
                        }
                    },
                    "expected_generation": {
                        "type": "string"
                    },
                    "tx": {
                        "type": "integer"
                    },
                    "max_bytes": {
                        "type": "integer"
                    }
                },
                "required": [
                    "source_store_id",
                    "graphs"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_knot_stage",
            "description": "Stage one immutable numbered snapshot part. Supply exact sha256 hashes and upload identity; replay is idempotent, changed bytes refuse. No file paths.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "upload_id": {
                        "type": "string"
                    },
                    "snapshot": {
                        "type": "string"
                    },
                    "content_hash": {
                        "type": "string"
                    },
                    "part_hash": {
                        "type": "string"
                    },
                    "payload": {
                        "type": "string"
                    },
                    "timestamp": {
                        "type": "string"
                    },
                    "actor": {
                        "type": "string"
                    },
                    "source": {
                        "type": "string"
                    },
                    "graph": {
                        "type": "string"
                    },
                    "total_parts": {
                        "type": "integer"
                    },
                    "total_bytes": {
                        "type": "integer"
                    },
                    "part_number": {
                        "type": "integer"
                    }
                },
                "required": [
                    "upload_id",
                    "snapshot",
                    "content_hash",
                    "part_hash",
                    "payload",
                    "total_parts",
                    "total_bytes",
                    "part_number"
                ],
                "additionalProperties": false
            }
        }),
        serde_json::json!({
            "name": "quipu_knot_promote",
            "description": "Start or read back atomic promotion of a staged snapshot upload. pending=true is not completion; poll the same upload_id. Existing validation gates remain authoritative.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "upload_id": {
                        "type": "string"
                    }
                },
                "required": [
                    "upload_id"
                ],
                "additionalProperties": false
            }
        }),
    ]
}
