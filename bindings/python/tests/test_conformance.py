"""Python runner for the shared bindings conformance corpus.

The same file drives the native runner (tests/bindings_conformance.rs at the
repository root). Matching rules are documented in the corpus itself.
"""

import json

import pytest

import quipu
from conftest import CORPUS


def substitute(value, env):
    if isinstance(value, str) and value.startswith("${") and value.endswith("}"):
        return env[value[2:-1]]
    if isinstance(value, list):
        return [substitute(v, env) for v in value]
    if isinstance(value, dict):
        return {k: substitute(v, env) for k, v in value.items()}
    return value


def matches(pattern, actual):
    if pattern == "$positive_int":
        return type(actual) is int and actual > 0
    if isinstance(pattern, dict) and "$contains" in pattern:
        return isinstance(actual, list) and all(any(matches(p, a) for a in actual) for p in pattern["$contains"])
    if isinstance(pattern, dict) and "$len" in pattern:
        return isinstance(actual, list) and len(actual) == pattern["$len"]
    if isinstance(pattern, dict):
        return isinstance(actual, dict) and all(k in actual and matches(v, actual[k]) for k, v in pattern.items())
    if isinstance(pattern, list):
        return isinstance(actual, list) and len(pattern) == len(actual) and all(map(matches, pattern, actual))
    # JSON scalars: compare type as well as value, so 1 != True and 1 != 1.0.
    return type(pattern) is type(actual) and pattern == actual


def pointer(doc, path):
    for part in path.lstrip("/").split("/"):
        doc = doc[int(part)] if isinstance(doc, list) else doc[part]
    return doc


def run_case(corpus, case, call):
    env = {f"shape:{k}": v for k, v in corpus["shapes"].items()}
    with quipu.Store.memory() as store:
        for i, step in enumerate(case["steps"]):
            where = f"{case['name']}[{i}] {step['op']}"
            payload = substitute(step["input"], env)
            expect = step["expect"]
            try:
                got = call(store, step["op"], payload)
            except quipu.QuipuError as e:
                assert "error" in expect, f"{where}: unexpected {e!r}"
                actual = {
                    "code": e.code,
                    "operation": e.operation,
                    "commit_state": e.commit_state,
                    "retryable": e.retryable,
                    "message": e.message,
                    "details": e.details,
                }
                assert matches(expect["error"], actual), f"{where}: {actual}"
                continue
            assert "ok" in expect, f"{where}: expected an error, got {got}"
            assert matches(expect["ok"], got), f"{where}: {got}"
            for name, path in step.get("capture", {}).items():
                env[name] = pointer(got, path)


def via_objects(store, op, payload):
    return store.invoke(op, payload)


def via_json_text(store, op, payload):
    return json.loads(store.invoke_json(op, json.dumps(payload)))


def _cases():
    corpus = json.loads(CORPUS.read_text())
    assert corpus["corpus"] == "quipu-bindings-conformance/v1"
    return [pytest.param(c, id=c["name"]) for c in corpus["cases"]]


@pytest.mark.parametrize("call", [via_objects, via_json_text], ids=["objects", "json"])
@pytest.mark.parametrize("case", _cases())
def test_shared_corpus(corpus, case, call):
    run_case(corpus, case, call)


def test_the_matcher_rejects_a_wrong_expectation(corpus):
    # Guard against a runner that passes everything: perturb one expectation.
    case = json.loads(json.dumps(corpus["cases"][1]))
    case["steps"][0]["expect"]["error"]["code"] = "not_the_code"
    with pytest.raises(AssertionError):
        run_case(corpus, case, via_objects)
