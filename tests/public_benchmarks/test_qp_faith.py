import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("replay", ROOT / "scripts/public_benchmarks/qp_faith.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class ReplayTests(unittest.TestCase):
    def setUp(self):
        self.probe = [{"query": "owl", "family": "A_entity", "known_slots": {}}]
        self.rows = [dict(self.probe[0], plan={})]

    def test_complete(self):
        m.validate_rows(self.rows, self.probe)

    def test_missing(self):
        with self.assertRaises(ValueError):
            m.validate_rows([], self.probe)

    def test_duplicate(self):
        with self.assertRaises(ValueError):
            m.validate_rows(self.rows * 2, self.probe)

    def test_unknown(self):
        self.rows[0]["query"] = "other"
        with self.assertRaises(ValueError):
            m.validate_rows(self.rows, self.probe)

    def test_family_mismatch(self):
        self.rows[0]["family"] = "B_attr"
        with self.assertRaises(ValueError):
            m.validate_rows(self.rows, self.probe)

    def test_label_mismatch(self):
        self.rows[0]["known_slots"] = {"product_type": "owl"}
        with self.assertRaises(ValueError):
            m.validate_rows(self.rows, self.probe)

    def test_missing_plan(self):
        del self.rows[0]["plan"]
        with self.assertRaises(ValueError):
            m.validate_rows(self.rows, self.probe)

    def test_duplicate_probe(self):
        with self.assertRaises(ValueError):
            m.validate_rows(self.rows, self.probe * 2)

    def test_corrupt_json(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "rows"
            p.write_text('{"query":')
            with self.assertRaises(ValueError):
                m.read_rows(p)

    def test_non_object_row(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "rows"
            p.write_text('[]')
            with self.assertRaises(ValueError):
                m.read_rows(p)

    def test_checksum(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp)
            (p / "data").write_bytes(b"test")
            m.verify(p, {"files": {"data": m.blob_sha(b"test")}})
            with self.assertRaises(ValueError):
                m.verify(p, {"files": {"data": m.blob_sha(b"other")}})

    def test_empty_denominator(self):
        with self.assertRaises(ValueError):
            m.score([], lambda p, s: False)

    def test_scoring_denominator(self):
        rows = self.rows + [{"family": "B_attr", "plan": {"product_type": "chair"}}]
        self.assertEqual(m.score(rows, lambda p, s: bool(p.get(s)))["category_routing_rate"], 0)


if __name__ == "__main__":
    unittest.main()
