#!/usr/bin/env python3
"""Offline graph display controls, including hostile labels and partial context."""
import copy
import importlib.util
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location("graph", Path(__file__).with_name("pendant-graph.py"))
graph = importlib.util.module_from_spec(spec)
spec.loader.exec_module(graph)


def edge(value="old", graph_id="g"):
    return dict(subject="s", subject_label="<script>alert(1)</script>", predicate="p",
                predicate_label="knows", object=value, object_label=value,
                graph=graph_id, graph_label=graph_id, multiplicity=1)


def report():
    return dict(summary="test", neighborhood=dict(touched=["s"],
                before=dict(edges=[edge()], total_edges=1, truncated=False),
                after=dict(edges=[edge("new")], total_edges=1, truncated=False)))


class GraphControls(unittest.TestCase):
    def test_changed_edges_are_visible_and_labels_cannot_execute(self):
        output = graph.render(report())
        self.assertIn('class="removed"', output)
        self.assertIn('class="added"', output)
        self.assertIn("&lt;script&gt;", output)
        self.assertNotIn("<script>", output)
        self.assertIn("default-src 'none'", output)
        self.assertIn("Alias caveat", output)
        self.assertEqual(output.count('<svg '), 2)
        self.assertIn('marker-end="url(#before)"', output)

    def test_unchanged_context_is_retained(self):
        data = report()
        data["neighborhood"]["after"] = copy.deepcopy(data["neighborhood"]["before"])
        self.assertEqual(graph.render(data).count('class="unchanged"'), 2)

    def test_graph_identity_changes_are_not_hidden_by_equal_labels(self):
        data = report()
        data["neighborhood"]["after"]["edges"] = [edge(graph_id="different")]
        output = graph.render(data)
        self.assertIn('class="removed"', output)
        self.assertIn('class="added"', output)

    def test_missing_edge_in_truncated_side_is_unknown(self):
        data = report()
        data["neighborhood"]["after"]["truncated"] = True
        output = graph.render(data)
        self.assertIn("TRUNCATED", output)
        self.assertIn('class="uncompared"', output)
        self.assertNotIn('class="removed"', output)

    def test_old_report_without_context_is_refused(self):
        with self.assertRaises(KeyError):
            graph.render({"summary": "old"})

    def test_diagram_has_shared_bounded_positions(self):
        data = report()
        before = data["neighborhood"]["before"]
        before["edges"] = [dict(edge(str(i)), subject=str(i)) for i in range(100)]
        after = data["neighborhood"]["after"]
        points = graph.positions(before, after, ["s"])
        self.assertEqual(len(points), 40)
        self.assertIn("s", points)
        self.assertEqual(points, graph.positions(after, before, ["s"]))


if __name__ == "__main__":
    unittest.main()
