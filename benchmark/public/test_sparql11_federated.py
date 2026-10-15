import importlib.util
import pathlib
import sys
import tempfile
import unittest
from unittest import mock

MODULE_PATH = pathlib.Path(__file__).with_name("sparql11_federated.py")
SPEC = importlib.util.spec_from_file_location("sparql11_federated", MODULE_PATH)
MODULE = importlib.util.module_from_spec(SPEC)
assert SPEC.loader
sys.modules[SPEC.name] = MODULE
SPEC.loader.exec_module(MODULE)


class FederatedManifestTests(unittest.TestCase):
    def test_discovers_service_data(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / "q.rq").write_text("SELECT * WHERE { SERVICE <http://e/query> { ?s ?p ?o } }")
            (root / "r.srx").write_text('<sparql xmlns="http://www.w3.org/2005/sparql-results#"><head/></sparql>')
            (root / "d.ttl").write_text("<s> <p> <o> .")
            (root / "manifest.ttl").write_text(''':x rdf:type mf:QueryEvaluationTest ; mf:name "x" ; dawgt:approval dawgt:Approved ; mf:action [ qt:query <q.rq> ; qt:serviceData [ qt:endpoint <http://e/query> ; qt:data <d.ttl> ] ] ; mf:result <r.srx> .''')
            cases = MODULE.discover(root / "manifest.ttl")
            self.assertEqual(cases[0][1], (("http://e/query", root / "d.ttl"),))


class ServerIsolationTests(unittest.TestCase):
    def test_retries_duplicate_port(self):
        with mock.patch.object(MODULE.EVAL, "reserve_port", side_effect=[1234, 1234, 5678]):
            urls = MODULE.endpoint_urls((("first", pathlib.Path("a")), ("second", pathlib.Path("b"))))
        self.assertEqual(urls, {"first": "http://127.0.0.1:1234/query", "second": "http://127.0.0.1:5678/query"})

    def test_duplicate_port_exhaustion_is_bounded(self):
        with mock.patch.object(MODULE.EVAL, "reserve_port", return_value=1234) as reserve:
            with self.assertRaisesRegex(ValueError, "distinct SERVICE ports"):
                MODULE.endpoint_urls((("first", pathlib.Path("a")), ("second", pathlib.Path("b"))))
        self.assertEqual(reserve.call_count, 33)

    def test_failed_child_cannot_borrow_healthy_listener(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            server = root / "server"
            server.write_text("#!/usr/bin/env python3\nimport sys\nprint('error binding: address in use', file=sys.stderr)\nsys.exit(7)\n")
            server.chmod(0o700)
            real_popen = MODULE.subprocess.Popen
            children = []
            def spawn_child(*args, **kwargs):
                child = real_popen(*args, **kwargs)
                children.append(child)
                return child
            with mock.patch.object(MODULE.subprocess, "Popen", side_effect=spawn_child):
                with mock.patch.object(MODULE.urllib.request, "urlopen") as health:
                    with self.assertRaisesRegex(ValueError, "address in use"):
                        MODULE.start_server(server, root, root / "db", "http://127.0.0.1:1234")
                    health.assert_not_called()
            self.assertEqual(children[0].returncode, 7)
            self.assertTrue(children[0].stderr.closed)


if __name__ == "__main__":
    unittest.main()
