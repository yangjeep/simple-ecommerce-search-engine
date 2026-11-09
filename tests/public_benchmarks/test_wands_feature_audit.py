import hashlib
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location(
    "audit", ROOT / "scripts/public_benchmarks/audit_wands_features.py")
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)


class FeatureAuditTests(unittest.TestCase):
    def test_repeated_values_are_preserved(self):
        self.assertEqual(m.features("color:white|color:black|material:wood"),
                         [("color", "white"), ("color", "black"), ("material", "wood")])

    def test_norm_matches_upstream(self):
        self.assertEqual(m.norm("White/Black"), "white black")

    def test_actual_decision_and_discarded_raw_value(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            raw, prepared, probe, native = [root / name for name in
                                           ("raw.csv", "catalog.jsonl", "probe.jsonl", "native.json")]
            raw.write_text("product_id\tproduct_name\tproduct_class\tproduct_features\n"
                           "1508\tmat\tDoormats\tcolor:white|color:black\n")
            prepared.write_text(json.dumps({"id": "1508", "color": "black"}) + "\n")
            probe.write_text(json.dumps({"query": "white doormats", "relevant_pids": [1508]}) + "\n")
            row = {"query": "white doormats", "family": "B_attr",
                   "retained_relevant_pids": [], "retained_relevant_products": 0,
                   "hard_enum_constraints": [{"attribute": "color", "value": "white"}]}
            summary = {"catalog_sha256": hashlib.sha256(prepared.read_bytes()).hexdigest(),
                       "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest()}
            native.write_text(json.dumps({"summary": summary, "rows": [row]}))
            result = m.audit(raw, prepared, probe, native)
            self.assertEqual(result["queries"][0]["enum_only_prediction_disagreement_ids"], [])
            self.assertEqual(result["queries"][0]["exclusion_reason_counts"],
                             {"matching_raw_value_not_prepared_exact_value": 1})
            self.assertFalse(result["focus_products"][0]["actual_native_retained"])
            prepared.write_text(prepared.read_text() + "\n")
            with self.assertRaisesRegex(ValueError, "native report input mismatch"):
                m.audit(raw, prepared, probe, native)


if __name__ == "__main__":
    unittest.main()
