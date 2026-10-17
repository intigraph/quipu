"""Build version-checked patches without replacing curator-owned prose."""


def resolve_ask_bindings(bindings, curated):
    """Resolve a legacy composite item using its explicit curated document ID."""
    if len(curated["asks"]) != len(curated["existing_doc_ids"]):
        raise ValueError("Every curated ask needs its explicit existing document ID")
    result = {**bindings, "asks": dict(bindings.get("asks", {}))}
    for ask, document_id in zip(curated["asks"], curated["existing_doc_ids"]):
        matches = [value for value in bindings.get("asks", {}).values()
                   if value["doc_id"] == document_id]
        if len(matches) != 1:
            raise ValueError("Missing or ambiguous actual version for curated document")
        result["asks"][ask["item"]] = matches[0]
    return result


def build_row_ops(priorities, asks, bindings):
    operations = []
    targets = set()
    for collection, rows in (("priorities", priorities), ("asks", asks)):
        for row in rows:
            binding = bindings.get(collection, {}).get(row["item"])
            if not binding or not isinstance(binding.get("doc_id"), str) or not binding["doc_id"]:
                raise ValueError("Missing actual document identity for " + row["item"])
            version = binding.get("version")
            if type(version) is not int or version < 1:
                raise ValueError("Missing actual document version for " + row["item"])
            target = (collection, binding["doc_id"])
            if target in targets:
                raise ValueError("Two rows target the same existing document")
            targets.add(target)
            fields = ("priority", "owner", "item", "rank") if collection == "priorities" else ("item",)
            operations.append({
                "op": "update", "collection": collection, "doc_id": binding["doc_id"],
                "if_version": version, "data": {field: row[field] for field in fields},
            })
    return operations
