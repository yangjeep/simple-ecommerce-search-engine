import importlib.util
from pathlib import Path
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "strict_tsv",
    Path(__file__).resolve().parents[2] / "scripts/public_benchmarks/strict_tsv.py",
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class StrictTsvTests(unittest.TestCase):
    def read(self, contents):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "input.tsv"
            path.write_text(contents, encoding="utf-8")
            return MODULE.read_rows(path)

    def test_preserves_nonpositional_ids_quotes_and_embedded_newlines(self):
        self.assertEqual(
            self.read('query_id\tquery\n42\t"hello\tworld\nquoted ""value"""\n'),
            [{"query_id": "42", "query": 'hello\tworld\nquoted "value"'}],
        )

    def test_rejects_duplicate_headers(self):
        with self.assertRaises(ValueError):
            self.read("id\tid\n1\t2\n")

    def test_rejects_wrong_width(self):
        with self.assertRaises(ValueError):
            self.read("id\tlabel\n1\n")

    def test_preserves_empty_fields(self):
        self.assertEqual(self.read("id\tvalue\n1\t\n"), [{"id": "1", "value": ""}])


if __name__ == "__main__":
    unittest.main()
