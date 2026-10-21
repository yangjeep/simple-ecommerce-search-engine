#!/usr/bin/env python3
"""Regression evidence for CSV-aware decoding of the pinned tab-delimited WANDS queries.
Runs entirely on bundled public source-record excerpts. Optional argument verifies
against the complete pinned official query.csv (not included in this source bundle).
"""
import csv
import hashlib
import io
import json
import pathlib
import sys
root = pathlib.Path(__file__).resolve().parents[1]
fixtures = root / 'crates/athos80-eval/tests/fixtures'
meta = json.loads((fixtures / 'wands_official_decoded_query_regressions.json').read_text())
legacy_path = fixtures / meta['legacy_fixture']
legacy = json.loads(legacy_path.read_text())
sha = lambda data: hashlib.sha256(data).hexdigest()
assert sha(legacy_path.read_bytes()) == meta['legacy_fixture_sha256']
expected = {str(c['query_id']): c['q'] for c in meta['cases']}
legacy_values = {str(c['query_id']): c['q'] for c in legacy['cases']}

def verify_bytes(data, label, expected_hash):
    assert sha(data) == expected_hash, label + ' source hash mismatch'
    # newline='' is intentional: csv, not ad-hoc splitting, owns field quoting.
    reader = csv.DictReader(io.StringIO(data.decode('utf-8'), newline=''), delimiter='\t')
    decoded = {r['query_id']: r['query'] for r in reader}
    serialized = {parts[0]: parts[1] for line in data.decode('utf-8').splitlines()[1:] if len(parts := line.split('\t')) >= 2}
    changed = sorted(int(k) for k in decoded if decoded[k] != serialized[k])
    assert changed == [208, 285, 391], (label, changed)
    for qid, query in expected.items():
        assert decoded[qid] == query, (label, qid, 'decoded mismatch')
        assert serialized[qid] == legacy_values[qid], (label, qid, 'legacy artifact mismatch')
        assert decoded[qid] != legacy_values[qid]
    return {'source': label, 'sha256': expected_hash, 'records': len(decoded), 'mismatched_naive_split_ids': changed, 'passed': True}

checks = [verify_bytes((fixtures / meta['source_excerpt']).read_bytes(), 'bundled exact official record excerpt', meta['source_excerpt_sha256'])]
if len(sys.argv) > 1:
    check = verify_bytes(pathlib.Path(sys.argv[1]).read_bytes(), 'complete pinned official query.csv', meta['source_sha256'])
    assert check['records'] == meta['source_record_count'] == 480
    checks.append(check)
print(json.dumps({'passed': True, 'reader': meta['reader'], 'checks': checks, 'runtime_implementation_changed': False}, indent=2))
