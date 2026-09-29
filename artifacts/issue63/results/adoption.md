# Issue #63 section 3.4 adoption (500k, median of 3 runs, in-process CPU/op)

| cand | correct (gate checks / failures) | best FH reduction | worst ratio (cell) | FH3+FH4 reduction µs | persistent B | passes |
|---|---|---|---|---|---|---|
| p0r | True (3312/0) | 95.7% | 1.044 (sh2_color_white_review_count_desc) | 8,527 | 0 | True |
| p1 | True (3312/0) | 95.2% | 1.100 (sc2_style_modern_average_rating_asc) | 8,226 | 65,608 | False |
| p2 | True (3312/0) | 99.4% | 1.185 (numeric_range_sort) | 17,409 | 65,608 | False |
| p2b | True (3312/0) | 99.2% | 1.203 (fc6_depth3_x_shape) | 19,349 | 65,608 | False |

**N+ = `p0r`** — 1 candidate(s) passed; largest FH3+FH4 reduction p0r; within-5% tie set ['p0r']; chosen p0r (smallest persistent memory 0 B, then larger reduction)

Per-cell ratios (cand ÷ P0):

| cell | p0r | p1 | p2 | p2b | P0 µs |
|---|---|---|---|---|---|
| facet_disjunctive_multi_dim | 0.707 | 0.729 | 0.403 | 0.383 | 14,824 |
| facet_high_cardinality_color | 0.686 | 0.685 | 0.359 | 0.237 | 13,360 |
| facet_low_cardinality_style | 0.043 | 0.048 | 0.006 | 0.008 | 4,883 |
| facet_medium_cardinality_primarymaterial | 0.099 | 0.091 | 0.029 | 0.033 | 5,408 |
| fc1_full_material | 0.071 | 0.070 | 0.019 | 0.019 | 5,170 |
| fc2_full_shape | 0.046 | 0.047 | 0.008 | 0.008 | 5,693 |
| fc3_style_modern_x_color | 0.969 | 0.978 | 0.959 | 1.149 | 1,714 |
| fc4_color_white_x_style | 0.948 | 0.935 | 0.956 | 1.021 | 253 |
| fc5_color_white_x_primarymaterial | 0.838 | 0.887 | 0.873 | 0.979 | 303 |
| fc6_depth3_x_shape | 0.983 | 0.956 | 0.932 | 1.203 | 60 |
| fc7_pergolas_x_color | 0.979 | 0.991 | 1.052 | 0.964 | 17 |
| numeric_range_sort | 1.014 | 1.052 | 1.185 | 1.113 | 4,009 |
| sc1_full_rating_count_desc | 0.007 | 0.003 | 0.003 | 0.003 | 4,469 |
| sc2_style_modern_average_rating_asc | 1.039 | 1.100 | 1.064 | 1.068 | 22 |
| sc3_accent_chairs_review_count_desc | 0.960 | 0.949 | 0.948 | 1.037 | 160 |
| sc4_color_black_rating_count_asc | 1.043 | 1.041 | 0.982 | 0.996 | 104 |
| sc5_pergolas_average_rating_desc | 0.953 | 0.966 | 1.007 | 0.972 | 19 |
| sc6_depth5_review_count_desc | 1.007 | 1.021 | 1.087 | 0.984 | 62 |
| sh2_color_white_review_count_desc | 1.044 | 1.013 | 1.181 | 1.013 | 78 |
| sh3_depth3_review_count_desc | 0.919 | 0.907 | 0.987 | 0.985 | 64 |
