import json, itertools, collections

CONFIGS = ["automotive", "wands_baseline", "wands_anonymized", "wands_noisy"]
BASE = "dataset_cache/export/e2b_llm_proposals_{config}_run{run}.json"

def load(config, run):
    with open(BASE.format(config=config, run=run)) as f:
        return json.load(f)

WANDS_SAMPLE_KEYS = [
    "overallproductweight","overallwidth-sidetoside","overallheight-toptobottom",
    "overalldepth-fronttoback","weightcapacity","estimatedtimetosetup",
    "commercialwarranty","adultassemblyrequired","organic","firerated",
    "drawersincluded","upholstered","installationrequired",
    "style","dsprimaryproductstyle","countryoforigin","levelofassembly",
    "dswoodtone","primarymaterial","framematerial","shape","pattern",
    "color","basecolor","finish","upholsterymaterial","upholsterycolor",
    "productwarranty","fullorlimitedwarranty","warrantylength","title",
    "productcare","piecesincluded","samplepartnumber",
    "compatibledrainassemblypartnumber","compatiblediningchairpartnumber",
]
anonymized_map = {f"feature_{i}": k for i, k in enumerate(sorted(WANDS_SAMPLE_KEYS))}
NOISY_PAIRS = [
    ("item_spec_7","overallproductweight"),("dim_a","overallwidth-sidetoside"),
    ("dim_b","overallheight-toptobottom"),("dim_c","overalldepth-fronttoback"),
    ("load_rating","weightcapacity"),("prep_index","estimatedtimetosetup"),
    ("flag_12","commercialwarranty"),("flag_13","adultassemblyrequired"),
    ("flag_14","organic"),("flag_15","firerated"),("flag_16","drawersincluded"),
    ("flag_17","upholstered"),("flag_18","installationrequired"),
    ("tag_group_a","style"),("tag_group_b","dsprimaryproductstyle"),
    ("origin_code","countryoforigin"),("assembly_flag","levelofassembly"),
    ("tone_code","dswoodtone"),("material_code","primarymaterial"),
    ("material_code_2","framematerial"),("form_code","shape"),("design_code","pattern"),
    ("hue_code","color"),("hue_code_2","basecolor"),("surface_code","finish"),
    ("fabric_code","upholsterymaterial"),("fabric_hue_code","upholsterycolor"),
    ("coverage_note","productwarranty"),("coverage_tier","fullorlimitedwarranty"),
    ("coverage_duration","warrantylength"),("label_alt","title"),
    ("handling_note","productcare"),("kit_manifest","piecesincluded"),
    ("product_code","samplepartnumber"),
    ("linked_product_code","compatibledrainassemblypartnumber"),
    ("linked_product_code_2","compatiblediningchairpartnumber"),
]
noisy_map = dict(NOISY_PAIRS)
REAL_KEY = {"automotive": lambda k: k, "wands_baseline": lambda k: k,
            "wands_anonymized": lambda k: anonymized_map[k], "wands_noisy": lambda k: noisy_map[k]}

all_data = {}
for cfg in CONFIGS:
    all_data[cfg] = {}
    for run in range(1, 6):
        d = load(cfg, run)
        all_data[cfg][run] = {desc["key"]: desc for desc in d["descriptors"]}

# category assignment per real key, based on manual analysis above
CATEGORY = {
    "basecolor": "primitive_selection",
    "color": "hallucination_error",   # dominant: spurious relationship read of junk placeholder + real data-quality noise
    "compatiblediningchairpartnumber": "scope_ambiguity",
    "compatibledrainassemblypartnumber": "insufficient_evidence",
    "dswoodtone": "scope_ambiguity",
    "finish": "primitive_selection",
    "framematerial": "scope_ambiguity",
    "heat_range": "primitive_selection",
    "overalldepth-fronttoback": "scope_ambiguity",
    "overallheight-toptobottom": "scope_ambiguity",
    "overallproductweight": "scope_ambiguity",
    "overallwidth-sidetoside": "scope_ambiguity",
    "primarymaterial": "primitive_selection",
    "productwarranty": "value_type_ambiguity",
    "samplepartnumber": "scope_ambiguity",
    "thread_size": "value_type_ambiguity",   # validator-boundary-artifact secondary
    "upholsterycolor": "primitive_selection",
    "upholsterymaterial": "primitive_selection",
    "voltage": "primitive_selection",
    "warrantylength": "value_type_ambiguity",
    "weightcapacity": "scope_ambiguity",
}

FIELDS = ["semantic_role", "value_type", "scope", "candidate_physical_primitive"]
pair_disagree_by_key_official = collections.Counter()   # role+primitive (the official metric)
pair_disagree_by_key_any = collections.Counter()        # any of the 4 fields
pair_disagree_by_key_scope_only = collections.Counter() # scope differs but role+primitive agree
total_pairs_all = 0
official_disagree_total = 0

for cfg in CONFIGS:
    runs = all_data[cfg]
    common_keys = set.intersection(*(set(runs[r].keys()) for r in runs))
    for r1, r2 in itertools.combinations(sorted(runs.keys()), 2):
        for k in common_keys:
            d1, d2 = runs[r1][k], runs[r2][k]
            real_k = REAL_KEY[cfg](k)
            total_pairs_all += 1
            official_disagree = (d1["semantic_role"] != d2["semantic_role"]) or (d1["candidate_physical_primitive"] != d2["candidate_physical_primitive"])
            any_disagree = official_disagree or (d1["scope"] != d2["scope"]) or (d1["value_type"] != d2["value_type"])
            if official_disagree:
                pair_disagree_by_key_official[real_k] += 1
                official_disagree_total += 1
            if any_disagree:
                pair_disagree_by_key_any[real_k] += 1
            if (not official_disagree) and (d1["scope"] != d2["scope"]):
                pair_disagree_by_key_scope_only[real_k] += 1

print(f"Total pairs: {total_pairs_all}, official (role+primitive) disagreeing pairs: {official_disagree_total} ({official_disagree_total/total_pairs_all*100:.2f}%)")
print(f"Official agreement rate: {(total_pairs_all-official_disagree_total)/total_pairs_all*100:.2f}%  <- matches 87.60%\n")

print("=== Category breakdown, weighted by OFFICIAL (role+primitive) disagreeing pairs ===")
cat_totals = collections.Counter()
for k, cnt in pair_disagree_by_key_official.items():
    cat = CATEGORY.get(k, "UNCATEGORIZED")
    cat_totals[cat] += cnt
for cat, cnt in cat_totals.most_common():
    print(f"  {cat:25s} {cnt:4d} pairs  ({cnt/official_disagree_total*100:5.1f}% of all official disagreement)")
print(f"  {'TOTAL':25s} {sum(cat_totals.values()):4d} pairs")

print("\n=== scope-only disagreement (role+primitive AGREE, scope alone differs) -- invisible to official metric ===")
scope_only_total = sum(pair_disagree_by_key_scope_only.values())
print(f"  {scope_only_total} pairs across {len([k for k,v in pair_disagree_by_key_scope_only.items() if v>0])} keys")
for k, cnt in pair_disagree_by_key_scope_only.most_common(25):
    if cnt > 0:
        print(f"    {k:35s} {cnt:3d}")

print(f"\n=== ANY-field disagreement (role/type/scope/primitive) total ===")
any_total = sum(pair_disagree_by_key_any.values())
print(f"  {any_total} / {total_pairs_all} pairs ({any_total/total_pairs_all*100:.2f}%) -- vs official {official_disagree_total/total_pairs_all*100:.2f}%")
