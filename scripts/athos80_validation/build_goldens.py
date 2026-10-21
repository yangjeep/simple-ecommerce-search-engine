#!/usr/bin/env python3
"""Freeze only passed independent HTTP observations for lightweight preflight."""
import argparse,copy,hashlib,json
from pathlib import Path

def sha(path):return hashlib.sha256(path.read_bytes()).hexdigest()
def canon(value):return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),allow_nan=False).encode()).hexdigest()
def main():
    ap=argparse.ArgumentParser();ap.add_argument('--run-directory',type=Path,required=True);ap.add_argument('--workload',type=Path,required=True);ap.add_argument('--output',type=Path,required=True);args=ap.parse_args()
    if args.output.exists():ap.error('Fresh output required')
    manifest=json.loads((args.run_directory/'manifest.json').read_text());suite=args.run_directory/'wands-all-arms';summary=json.loads((suite/'summary.json').read_text())
    assert manifest['passed'] and manifest['frozen_inputs_unchanged'] and not manifest['owned_listener_remaining']
    assert summary['passed'] and summary['failed_checks']==0 and summary['cases_sha256']==sha(args.workload)
    workload=json.loads(args.workload.read_text());caseids={case['id'] for case in workload['cases']};arms=['base','merch','personal','both'];entries={arm:{} for arm in arms}
    for line in (suite/'validated_responses.jsonl').read_text().splitlines():
        row=json.loads(line);arm=row['arm'];caseid=row['id'];assert arm in arms and caseid in caseids and caseid not in entries[arm]
        stable=copy.deepcopy(row['response']);del stable['metrics']['elapsed_micros']
        assert row['request_sha256']==canon(row['request']) and row['stable_response_sha256']==canon(stable)
        entries[arm][caseid]={'request_sha256':row['request_sha256'],'stable_response_sha256':row['stable_response_sha256']}
    assert all(set(records)==caseids for records in entries.values())
    assert sum(map(len,entries.values()))==summary['checks']
    golden={'schema':'athos80-http-goldens-v1','workload_sha256':sha(args.workload),'catalog_sha256':manifest['catalog_sha256'],
            'binary_sha256':manifest['binary_sha256'],'oracle_summary_sha256':sha(suite/'summary.json'),
            'normalization':'remove metrics.elapsed_micros only','entries':entries,
            'independent_fields':['contract','lexical_profile','result_mode','facet_count_mode','num_found','all hit fields','facets','active_rule_ids','metrics.eligible_variants','metrics.eligible_products','metrics.lexical_candidates','metrics.ranked_variants'],
            'additional_native_stability_fields':['diagnostics','metrics.catalog_variants','metrics.structural_candidates','metrics.verified_variants','metrics.facet_verified_variants'],
            'provenance':{'run_manifest_sha256':sha(args.run_directory/'manifest.json'),'validated_responses_sha256':sha(suite/'validated_responses.jsonl'),'independent_expected_sha256':sha(suite/'expected.jsonl')}}
    args.output.write_text(json.dumps(golden,sort_keys=True,indent=2)+'\n')
    print(json.dumps({'golden':str(args.output),'sha256':sha(args.output),'cases_per_arm':len(caseids),'arms':arms},indent=2))
if __name__=='__main__':main()
