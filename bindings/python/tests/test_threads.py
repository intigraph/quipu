"""Thread affinity is an explicit limitation, enforced and tested.

A handle refuses every call from a thread other than its opener, before the
engine is entered. This is not a concurrency promise and makes no GIL-safety
claim; open one store per thread.
"""

import threading

import pytest

import quipu


def run_in_thread(fn):
    box = {}

    def target():
        try:
            box["value"] = fn()
        except BaseException as e:  # noqa: BLE001 - surface anything to the test
            box["error"] = e

    t = threading.Thread(target=target)
    t.start()
    t.join()
    return box


@pytest.mark.parametrize(
    "call",
    [
        lambda s: s.query("ASK { ?s ?p ?o }"),
        lambda s: s.capabilities(),
        lambda s: s.load_shapes("x", "@prefix sh: <http://www.w3.org/ns/shacl#> ."),
        lambda s: s.invoke_json("quipu_query", '{"query": "ASK { ?s ?p ?o }"}'),
        lambda s: s.close(),
    ],
)
def test_cross_thread_calls_are_refused_and_the_owner_keeps_working(call):
    with quipu.Store.memory() as s:
        box = run_in_thread(lambda: call(s))
        err = box.get("error")
        assert isinstance(err, quipu.ThreadAffinityError), box
        assert err.code == "thread_affinity"
        assert err.commit_state == "not_started"
        assert err.retryable is False
        assert not s.closed
        assert s.query("ASK { ?s ?p ?o }") is False


def test_a_store_per_thread_works():
    box = run_in_thread(lambda: quipu.Store.memory().query("ASK { ?s ?p ?o }"))
    assert box == {"value": False}


def test_a_handle_dropped_on_another_thread_does_not_crash():
    s = quipu.Store.memory()
    holder = [s]
    del s
    box = run_in_thread(lambda: holder.pop() and None)
    assert "error" not in box
