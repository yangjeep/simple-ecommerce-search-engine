import pandas as pd
import re
from collections import Counter

df = pd.read_csv("dataset_cache/wands/product.csv", sep="\t")
print("=== shape ===")
print(df.shape)
print(df.columns.tolist())

print("\n=== null counts ===")
print(df.isna().sum())

print("\n=== product_class ===")
print("distinct product_class:", df["product_class"].nunique())
print(df["product_class"].value_counts().head(20))

print("\n=== category hierarchy depth distribution ===")
def depth(s):
    if pd.isna(s):
        return 0
    return len([p for p in s.split(" / ") if p.strip()])

df["_depth"] = df["category hierarchy"].apply(depth)
print(df["_depth"].value_counts().sort_index())

print("\n=== distinct hierarchy prefixes per depth ===")
def prefixes(s, d):
    if pd.isna(s):
        return None
    parts = [p.strip() for p in s.split(" / ") if p.strip()]
    if len(parts) < d:
        return None
    return " / ".join(parts[:d])

for d in range(1, 8):
    col = df["category hierarchy"].apply(lambda s: prefixes(s, d))
    n = col.nunique()
    covered = col.notna().sum()
    print(f"depth {d}: distinct nodes={n}, products with this depth available={covered}")

print("\n=== product_class vs deepest hierarchy segment: how often do they match? ===")
def leaf(s):
    if pd.isna(s):
        return None
    parts = [p.strip() for p in s.split(" / ") if p.strip()]
    return parts[-1] if parts else None

df["_leaf"] = df["category hierarchy"].apply(leaf)
match = (df["_leaf"] == df["product_class"]).sum()
print(f"leaf==product_class: {match} / {len(df)} ({100*match/len(df):.1f}%)")

def second_last(s):
    if pd.isna(s):
        return None
    parts = [p.strip() for p in s.split(" / ") if p.strip()]
    return parts[-2] if len(parts) >= 2 else None

df["_second_last"] = df["category hierarchy"].apply(second_last)
match2 = (df["_second_last"] == df["product_class"]).sum()
print(f"second_last==product_class: {match2} / {len(df)} ({100*match2/len(df):.1f}%)")

print("\n=== product_features: key frequency analysis ===")
key_counter = Counter()
sample_n = 0
for feats in df["product_features"].dropna():
    sample_n += 1
    for kv in str(feats).split("|"):
        if ":" in kv:
            k = kv.split(":", 1)[0].strip()
            key_counter[k] += 1

print(f"products with any features: {sample_n} / {len(df)}")
print("top 40 most common feature keys:")
for k, c in key_counter.most_common(40):
    print(f"  {k!r}: {c} ({100*c/sample_n:.1f}%)")

print("\n=== looking for brand/manufacturer/store/price-like keys ===")
candidates = [k for k in key_counter if re.search(r"brand|manufactur|store|vendor|price|cost|msrp", k, re.I)]
for k in candidates:
    print(f"  {k!r}: {key_counter[k]}")
if not candidates:
    print("  NONE FOUND")

print("\n=== looking for a parent/variant-like id column (already know columns above, double-check) ===")
print([c for c in df.columns if "parent" in c.lower() or "variant" in c.lower() or "asin" in c.lower()])

print("\n=== rating/review stats (as availability/quality proxy, not price) ===")
print(df[["rating_count", "average_rating", "review_count"]].describe())
