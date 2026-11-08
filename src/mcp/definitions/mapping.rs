//! C1 tools keep read-only preparation separate from the lifecycle writer.
use serde_json::{Value, json};

pub(super) fn defs() -> Vec<Value> {
    let row = json!({"type":"object","additionalProperties":false,
        "properties":{
            "subject_id":{"type":"string"},"predicate_id":{"type":"string","enum":[
                "http://www.w3.org/2002/07/owl#equivalentClass",
                "http://www.w3.org/2002/07/owl#equivalentProperty",
                "http://www.w3.org/2002/07/owl#inverseOf",
                "http://www.w3.org/2000/01/rdf-schema#subClassOf",
                "http://www.w3.org/2000/01/rdf-schema#subPropertyOf"]},
            "object_id":{"type":"string"},"mapping_justification":{"type":"string"},
            "author_id":{"type":"string"},"quipu_review":{"type":"string"}},
        "required":["subject_id","predicate_id","object_id","mapping_justification","author_id","quipu_review"]});
    let mapping = json!({"type":"object","additionalProperties":false,
        "properties":{
            "id":{"type":"string"},"graph":{"type":"string"},
            "maps_from":{"type":"array","minItems":1,"maxItems":64,"uniqueItems":true,"items":{"type":"string"}},
            "maps_to":{"type":"array","minItems":1,"maxItems":64,"uniqueItems":true,"items":{"type":"string"}},
            "pins":{"type":"array","minItems":1,"maxItems":128,"uniqueItems":true,"items":{
                "type":"object","additionalProperties":false,"properties":{
                    "registry":{"type":"string","enum":["shapes","ontologies"]},
                    "name":{"type":"string"},"valid_from":{"type":"string"},"digest":{"type":"string"}},
                "required":["registry","name","valid_from","digest"]}},
            "rows":{"type":"array","minItems":1,"maxItems":1024,"uniqueItems":true,"items":row}},
        "required":["id","graph","maps_from","maps_to","pins","rows"]});
    let proposal = json!({"type":"object","additionalProperties":false,
        "properties":{"mapping":mapping,"expected_digest":{"type":"string"}},
        "required":["mapping","expected_digest"]});
    vec![
        json!({"name":"quipu_mapping_propose",
            "description":"READ. Qualify explicit one-hop schema correspondences, existing named scope and exact current shape/ontology pins. Returns a digest-bound proposal. Creates no graphs or transactions and performs no automatic matching.",
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false},
            "inputSchema":{"type":"object","additionalProperties":false,
                "properties":{"mapping":mapping},"required":["mapping"]}}),
        json!({"name":"quipu_mapping_decide",
            "description":"READ. Pure explicit per-row accept/decline decisions bound to the whole proposal digest. Every full correspondence key needs exactly one decision. Returns the selected mapping and expected_digest; declared reviewer is provenance, not a credential.",
            "annotations":{"readOnlyHint":true,"destructiveHint":false,"openWorldHint":false},
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{
                "proposal":proposal,"reviewer":{"type":"string"},
                "decisions":{"type":"array","minItems":1,"maxItems":1024,"items":{
                    "type":"object","additionalProperties":false,"properties":{
                        "subject_id":{"type":"string"},"predicate_id":{"type":"string"},
                        "object_id":{"type":"string"},"accept":{"type":"boolean"}},
                    "required":["subject_id","predicate_id","object_id","accept"]}}},
                "required":["proposal","reviewer","decisions"]}}),
        json!({"name":"quipu_mapping_apply",
            "description":"WRITE. Apply or retire a decided mapping with exact expected_digest. Both graphs must already exist; existing graph authority/validation gates remain. Records the actual lifecycle decision and MappingRecord in the same tx. Exact replay does not reopen retirement. No consumer or replica activation.",
            "annotations":{"readOnlyHint":false,"destructiveHint":true,"openWorldHint":false},
            "inputSchema":{"type":"object","additionalProperties":false,"properties":{
                "action":{"type":"string","enum":["apply","retire"]},
                "mapping":{"description":"Decided mapping object for apply, mapping IRI string for retire.","oneOf":[mapping,{"type":"string"}]},
                "expected_digest":{"type":"string"},"actor":{"type":"string"},
                "operations_graph":{"type":"string","description":"Existing committed audit graph; default urn:quipu:graph:operations. Only for apply."}},
                "required":["action","mapping","expected_digest","actor"]}}),
    ]
}
