#!/usr/bin/env python3
"""Lossless TSV-to-JSON rows for the ranked evaluator; Python stdlib only.

Used for WANDS query/label exports, not products. Reject malformed widths and
repeated headers rather than dropping rows. CSV quoting/newlines are honored.
"""
import csv
import json
import sys


def read_rows(path):
    with open(path, encoding="utf-8", newline="") as handle:
        reader = csv.reader(handle, delimiter="\t", strict=True)
        header = next(reader)
        if not header or len(set(header)) != len(header):
            raise ValueError("empty or duplicate TSV headers")
        rows = []
        for row in reader:
            if len(row) != len(header):
                raise ValueError(f"TSV row {reader.line_num}: wrong column count")
            rows.append(dict(zip(header, row)))
        return rows


if __name__ == "__main__":
    if len(sys.argv) != 2:
        raise SystemExit("usage: strict_tsv.py <input.tsv>")
    json.dump(read_rows(sys.argv[1]), sys.stdout, ensure_ascii=False)
