# Issue #63 microbenchmarks (median of run medians; in-process thread CPU)

## Tier 100k (3 runs, 128,982 ordinals)

### B1 construction

| rep | CPU µs/op (runs) | allocs/op | bytes/op | bytes touched est. | persistent B |
|---|---|---|---|---|---|
| p0 | 1,154.26 (1,154.3 / 1,182.2 / 1,104.0) | 27.0 | 82,032 | 32,768 | 0 |
| p0r | 3.70 (3.7 / 3.7 / 3.7) | 3.0 | 16,512 | 16,384 | 0 |
| p1 | 0.00 (0.0 / 0.0 / 0.0) | 0.0 | 0 | 0 | 16,408 |
| p2 | 0.00 (0.0 / 0.0 / 0.0) | 0.0 | 0 | 0 | 0 |

### B1 pipeline (FH cells)

| cell | cand | CPU µs/op | allocs/op | phases µs cand/facets/sort | paths |
|---|---|---|---|---|---|
| facet_disjunctive_multi_dim | p0 | 4,858 | 3459 | 12 / 4,974 / 50 | ordinal,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p0r | 3,781 | 3436 | 8 / 3,398 / 52 | ordinal,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p1 | 3,711 | 3433 | 7 / 3,029 / 36 | ordinal,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p2 | 2,373 | 3433 | 7 / 2,106 / 34 | ordinal_all,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p2b | 2,821 | 3438 | 7 / 2,260 / 37 | bitmap_all,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_high_cardinality_color | p0 | 4,263 | 3292 | 1,081 / 2,768 / 47 | ordinal bounded_unsorted |
| facet_high_cardinality_color | p0r | 2,979 | 3269 | 7 / 3,181 / 70 | ordinal bounded_unsorted |
| facet_high_cardinality_color | p1 | 3,045 | 3266 | 1 / 3,424 / 74 | ordinal bounded_unsorted |
| facet_high_cardinality_color | p2 | 1,910 | 3266 | 0 / 1,781 / 32 | ordinal_all bounded_unsorted_all |
| facet_high_cardinality_color | p2b | 2,521 | 3271 | 0 / 1,785 / 31 | bitmap_all bounded_unsorted_all |
| facet_low_cardinality_style | p0 | 1,268 | 162 | 1,051 / 51 / 21 | bitmap bounded_unsorted |
| facet_low_cardinality_style | p0r | 72 | 139 | 4 / 40 / 17 | bitmap bounded_unsorted |
| facet_low_cardinality_style | p1 | 72 | 136 | 0 / 68 / 27 | bitmap bounded_unsorted |
| facet_low_cardinality_style | p2 | 30 | 136 | 0 / 17 / 8 | bitmap_all bounded_unsorted_all |
| facet_low_cardinality_style | p2b | 31 | 136 | 0 / 16 / 9 | bitmap_all bounded_unsorted_all |
| facet_medium_cardinality_primarymaterial | p0 | 2,573 | 365 | 1,086 / 1,422 / 33 | ordinal bounded_unsorted |
| facet_medium_cardinality_primarymaterial | p0r | 1,538 | 342 | 5 / 1,371 / 25 | ordinal bounded_unsorted |
| facet_medium_cardinality_primarymaterial | p1 | 1,514 | 339 | 0 / 1,446 / 31 | ordinal bounded_unsorted |
| facet_medium_cardinality_primarymaterial | p2 | 539 | 339 | 0 / 499 / 17 | ordinal_all bounded_unsorted_all |
| facet_medium_cardinality_primarymaterial | p2b | 122 | 342 | 0 / 95 / 12 | bitmap_all bounded_unsorted_all |

### C primitives

| primitive | variant | CPU ns/op | allocs/op | bytes/op | bytes touched est. | result |
|---|---|---|---|---|---|---|
| conjunction_3way | indexed_candidates | 9,824 | 15.0 | 41,644 | 41,414 | 9 |
| conjunction_5way | indexed_candidates | 989,617 | 47.0 | 131,815 | 1,583,882 | 3 |
| exact_lookup | variant_id_to_ordinal_and_record | 197,542 | 0.0 | 0 | 256,000 | 1,000 |
| lexical_and:outdoor+dining+table | lexical_and_candidates | 11,227 | 11.0 | 50,706 | 49,152 | 279 |
| lexical_and:wood+bed | lexical_and_candidates | 14,052 | 8.0 | 39,112 | 32,768 | 3,108 |
| numeric_range_broad_rating_gte_4 | indexed_candidates | 940,706 | 27.0 | 82,032 | 1,550,800 | 95,901 |
| numeric_range_narrow_review_count_p99 | indexed_candidates | 59,085 | 18.0 | 6,256 | 18,144 | 1,008 |
| same_variant_conjunction | indexed_candidates | 55,652 | 30.0 | 163,857 | 163,316 | 16,122 |
| single_bitmap_category_accent_chairs | indexed_candidates | 366 | 3.0 | 6,682 | 13,236 | 3,309 |
| single_bitmap_color_white | borrowed_len | 4 | 0.0 | 0 | 0 | 4,314 |
| single_bitmap_color_white | indexed_candidates | 539 | 5.0 | 8,702 | 17,256 | 4,314 |

## Tier 500k (3 runs, 515,928 ordinals)

### B1 construction

| rep | CPU µs/op (runs) | allocs/op | bytes/op | bytes touched est. | persistent B |
|---|---|---|---|---|---|
| p0 | 4,482.01 (5,168.6 / 4,482.0 / 4,279.2) | 106.0 | 328,000 | 131,072 | 0 |
| p0r | 16.59 (16.6 / 17.8 / 15.9) | 10.0 | 65,920 | 65,536 | 0 |
| p1 | 0.01 (0.0 / 0.0 / 0.0) | 0.0 | 0 | 0 | 65,608 |
| p2 | 0.00 (0.0 / 0.0 / 0.0) | 0.0 | 0 | 0 | 0 |

### B1 pipeline (FH cells)

| cell | cand | CPU µs/op | allocs/op | phases µs cand/facets/sort | paths |
|---|---|---|---|---|---|
| facet_disjunctive_multi_dim | p0 | 14,824 | 3544 | 19 / 16,392 / 104 | ordinal,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p0r | 10,487 | 3449 | 29 / 16,538 / 143 | ordinal,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p1 | 10,806 | 3439 | 18 / 9,562 / 93 | ordinal,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p2 | 5,975 | 3439 | 15 / 5,693 / 91 | ordinal_all,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_disjunctive_multi_dim | p2b | 5,673 | 3444 | 17 / 4,971 / 89 | bitmap_all,ordinal,ordinal,ordinal,ordinal bounded_unsorted |
| facet_high_cardinality_color | p0 | 13,360 | 3371 | 4,252 / 7,529 / 80 | ordinal bounded_unsorted |
| facet_high_cardinality_color | p0r | 9,170 | 3276 | 23 / 7,590 / 75 | ordinal bounded_unsorted |
| facet_high_cardinality_color | p1 | 9,151 | 3266 | 1 / 7,464 / 74 | ordinal bounded_unsorted |
| facet_high_cardinality_color | p2 | 4,800 | 3266 | 1 / 3,661 / 54 | ordinal_all bounded_unsorted_all |
| facet_high_cardinality_color | p2b | 3,161 | 3271 | 1 / 2,437 / 50 | bitmap_all bounded_unsorted_all |
| facet_low_cardinality_style | p0 | 4,883 | 241 | 4,303 / 244 / 52 | bitmap bounded_unsorted |
| facet_low_cardinality_style | p0r | 208 | 146 | 16 / 134 / 24 | bitmap bounded_unsorted |
| facet_low_cardinality_style | p1 | 233 | 136 | 0 / 148 / 28 | bitmap bounded_unsorted |
| facet_low_cardinality_style | p2 | 30 | 136 | 0 / 19 / 10 | bitmap_all bounded_unsorted_all |
| facet_low_cardinality_style | p2b | 37 | 136 | 0 / 18 / 8 | bitmap_all bounded_unsorted_all |
| facet_medium_cardinality_primarymaterial | p0 | 5,408 | 447 | 4,495 / 804 / 75 | bitmap bounded_unsorted |
| facet_medium_cardinality_primarymaterial | p0r | 535 | 352 | 21 / 501 / 44 | bitmap bounded_unsorted |
| facet_medium_cardinality_primarymaterial | p1 | 493 | 342 | 0 / 471 / 38 | bitmap bounded_unsorted |
| facet_medium_cardinality_primarymaterial | p2 | 157 | 342 | 0 / 136 / 19 | bitmap_all bounded_unsorted_all |
| facet_medium_cardinality_primarymaterial | p2b | 177 | 342 | 0 / 187 / 29 | bitmap_all bounded_unsorted_all |

### C primitives

| primitive | variant | CPU ns/op | allocs/op | bytes/op | bytes touched est. | result |
|---|---|---|---|---|---|---|
| conjunction_3way | indexed_candidates | 42,018 | 33.0 | 166,408 | 165,656 | 36 |
| conjunction_5way | indexed_candidates | 4,329,846 | 150.0 | 526,931 | 6,335,528 | 12 |
| exact_lookup | variant_id_to_ordinal_and_record | 229,462 | 0.0 | 0 | 256,000 | 1,000 |
| lexical_and:outdoor+dining+table | lexical_and_candidates | 49,926 | 34.0 | 201,816 | 196,238 | 1,116 |
| lexical_and:wood+bed | lexical_and_candidates | 110,512 | 26.0 | 156,448 | 131,072 | 12,432 |
| numeric_range_broad_rating_gte_4 | indexed_candidates | 3,875,616 | 106.0 | 328,000 | 6,203,200 | 383,604 |
| numeric_range_narrow_review_count_p99 | indexed_candidates | 275,986 | 69.0 | 22,848 | 72,576 | 4,032 |
| single_bitmap_category_accent_chairs | indexed_candidates | 2,423 | 9.0 | 26,728 | 52,944 | 13,236 |
| single_bitmap_color_white | borrowed_len | 8 | 0.0 | 0 | 0 | 17,256 |
| single_bitmap_color_white | indexed_candidates | 2,909 | 11.0 | 34,778 | 69,024 | 17,256 |

### B2 facet counting (ns per candidate unless noted)

| set | attr | V | |C| | ordinal | bitmap (ns/value) | d1 iter | d2 +gather | d3 +count u64 | d3 u32 | d5 materialize (ns/value) | dense ord | dense bitmap-len (ns/value) |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| full | color | 2825 | 515,928 | 15.38 | 2313.1 | 8.12 | 8.42 | 12.61 | 12.57 | 478.3 | 7.36 | 921.9 |
| full | material | 162 | 515,928 | 10.05 | 1869.8 | 8.12 | 8.76 | 10.91 | 10.45 | 234.3 | 2.75 | 379.3 |
| full | primarymaterial | 244 | 515,928 | 11.20 | 1757.5 | 8.12 | 8.41 | 11.48 | 11.47 | 277.2 | 3.40 | 462.5 |
| full | shape | 94 | 515,928 | 10.16 | 2162.3 | 8.12 | 8.48 | 11.56 | 10.48 | 220.7 | 3.02 | 427.9 |
| full | style | 65 | 515,928 | 11.51 | 2088.6 | 8.12 | 9.26 | 12.20 | 11.65 | 179.7 | 3.72 | 330.6 |
| prefix:0.001 | color | 2825 | 515 | 50.08 | 1469.9 | 7.29 | 7.61 | 12.50 | 10.04 | 6.6 | — | — |
| prefix:0.001 | material | 162 | 515 | 17.15 | 1115.6 | 7.29 | 7.76 | 10.66 | 10.09 | 20.9 | — | — |
| prefix:0.001 | primarymaterial | 244 | 515 | 18.20 | 1189.2 | 7.29 | 7.86 | 9.79 | 9.46 | 14.6 | — | — |
| prefix:0.001 | shape | 94 | 515 | 12.41 | 1043.6 | 7.29 | 7.64 | 9.31 | 8.90 | 14.5 | — | — |
| prefix:0.001 | style | 65 | 515 | 12.78 | 1146.0 | 7.29 | 8.45 | 10.74 | 9.30 | 22.2 | — | — |
| prefix:0.003 | color | 2825 | 1,547 | 49.75 | 3337.6 | 7.13 | 7.61 | 12.88 | 10.70 | 20.0 | — | — |
| prefix:0.003 | material | 162 | 1,547 | 13.06 | 2865.0 | 7.13 | 8.65 | 9.90 | 9.30 | 40.6 | — | — |
| prefix:0.003 | primarymaterial | 244 | 1,547 | 15.44 | 3180.9 | 7.13 | 7.96 | 10.29 | 9.62 | 33.6 | — | — |
| prefix:0.003 | shape | 94 | 1,547 | 12.21 | 2930.7 | 7.13 | 7.98 | 10.23 | 9.28 | 43.0 | — | — |
| prefix:0.003 | style | 65 | 1,547 | 12.71 | 2936.7 | 7.13 | 8.22 | 10.03 | 9.72 | 45.6 | — | — |
| prefix:0.01 | color | 2825 | 5,159 | 46.42 | 556.7 | 8.00 | 8.95 | 12.56 | 12.33 | 59.2 | — | — |
| prefix:0.01 | material | 162 | 5,159 | 12.36 | 345.3 | 8.00 | 8.46 | 10.41 | 10.35 | 71.4 | — | — |
| prefix:0.01 | primarymaterial | 244 | 5,159 | 14.73 | 429.6 | 8.00 | 8.60 | 11.47 | 10.72 | 76.9 | — | — |
| prefix:0.01 | shape | 94 | 5,159 | 12.98 | 407.0 | 8.00 | 8.82 | 11.44 | 10.70 | 84.5 | — | — |
| prefix:0.01 | style | 65 | 5,159 | 11.72 | 385.5 | 8.00 | 8.75 | 12.14 | 11.49 | 79.5 | — | — |
| prefix:0.03 | color | 2825 | 15,477 | 45.83 | 708.8 | 7.88 | 8.35 | 12.02 | 12.09 | 187.4 | — | — |
| prefix:0.03 | material | 162 | 15,477 | 12.60 | 436.2 | 7.88 | 8.40 | 10.32 | 10.18 | 123.2 | — | — |
| prefix:0.03 | primarymaterial | 244 | 15,477 | 12.56 | 481.1 | 7.88 | 8.50 | 11.01 | 10.51 | 145.4 | — | — |
| prefix:0.03 | shape | 94 | 15,477 | 10.89 | 454.7 | 7.88 | 8.80 | 10.59 | 12.16 | 129.8 | — | — |
| prefix:0.03 | style | 65 | 15,477 | 10.92 | 418.5 | 7.88 | 8.50 | 10.92 | 10.83 | 110.9 | — | — |
| prefix:0.1 | color | 2825 | 51,592 | 40.41 | 966.0 | 7.84 | 8.31 | 12.71 | 12.95 | 457.5 | — | — |
| prefix:0.1 | material | 162 | 51,592 | 10.73 | 574.5 | 7.84 | 8.92 | 10.26 | 10.42 | 251.1 | — | — |
| prefix:0.1 | primarymaterial | 244 | 51,592 | 12.14 | 650.0 | 7.84 | 8.92 | 10.79 | 10.74 | 279.1 | — | — |
| prefix:0.1 | shape | 94 | 51,592 | 10.81 | 585.9 | 7.84 | 8.31 | 11.01 | 10.58 | 200.3 | — | — |
| prefix:0.1 | style | 65 | 51,592 | 11.10 | 518.8 | 7.84 | 8.36 | 10.74 | 10.97 | 171.8 | — | — |
| prefix:0.25 | color | 2825 | 128,982 | 25.59 | 1316.9 | 7.80 | 8.26 | 12.16 | 11.55 | 467.3 | — | — |
| prefix:0.25 | material | 162 | 128,982 | 10.25 | 700.9 | 7.80 | 8.40 | 10.51 | 10.32 | 232.4 | — | — |
| prefix:0.25 | primarymaterial | 244 | 128,982 | 11.43 | 865.3 | 7.80 | 8.45 | 10.52 | 11.56 | 261.3 | — | — |
| prefix:0.25 | shape | 94 | 128,982 | 10.89 | 736.6 | 7.80 | 8.58 | 12.14 | 10.79 | 207.7 | — | — |
| prefix:0.25 | style | 65 | 128,982 | 10.72 | 710.2 | 7.80 | 8.49 | 10.79 | 10.84 | 205.5 | — | — |
| prefix:0.5 | color | 2825 | 257,964 | 18.02 | 1817.9 | 7.76 | 8.50 | 12.38 | 11.68 | 457.6 | — | — |
| prefix:0.5 | material | 162 | 257,964 | 11.04 | 1235.5 | 7.76 | 8.91 | 10.13 | 10.18 | 235.8 | — | — |
| prefix:0.5 | primarymaterial | 244 | 257,964 | 10.96 | 1132.6 | 7.76 | 8.32 | 10.76 | 11.00 | 332.0 | — | — |
| prefix:0.5 | shape | 94 | 257,964 | 10.12 | 1116.1 | 7.76 | 8.52 | 11.44 | 10.82 | 232.8 | — | — |
| prefix:0.5 | style | 65 | 257,964 | 10.66 | 1132.4 | 7.76 | 8.36 | 11.20 | 10.80 | 175.1 | — | — |
| random:0.001 | color | 2825 | 495 | 67.01 | 3831.5 | 7.53 | 8.12 | 15.39 | 10.81 | 9.1 | — | — |
| random:0.001 | material | 162 | 495 | 18.60 | 4660.3 | 7.53 | 8.20 | 11.67 | 10.67 | 25.7 | — | — |
| random:0.001 | primarymaterial | 244 | 495 | 23.31 | 4092.6 | 7.53 | 8.03 | 12.21 | 11.04 | 22.5 | — | — |
| random:0.001 | shape | 94 | 495 | 17.25 | 5531.8 | 7.53 | 8.00 | 11.41 | 10.88 | 31.6 | — | — |
| random:0.001 | style | 65 | 495 | 14.71 | 4894.8 | 7.53 | 8.63 | 14.41 | 10.59 | 18.4 | — | — |
| random:0.003 | color | 2825 | 1,595 | 56.82 | 5624.5 | 7.23 | 7.92 | 15.13 | 12.87 | 22.1 | — | — |
| random:0.003 | material | 162 | 1,595 | 15.92 | 7903.8 | 7.23 | 7.95 | 11.35 | 10.57 | 48.6 | — | — |
| random:0.003 | primarymaterial | 244 | 1,595 | 20.04 | 6427.7 | 7.23 | 8.46 | 12.46 | 13.09 | 40.0 | — | — |
| random:0.003 | shape | 94 | 1,595 | 13.09 | 9211.0 | 7.23 | 8.16 | 12.17 | 11.72 | 33.4 | — | — |
| random:0.003 | style | 65 | 1,595 | 17.96 | 8492.5 | 7.23 | 7.92 | 15.09 | 13.58 | 48.0 | — | — |
| random:0.01 | color | 2825 | 5,193 | 63.21 | 10543.3 | 7.12 | 8.51 | 15.39 | 15.30 | 70.3 | — | — |
| random:0.01 | material | 162 | 5,193 | 15.62 | 14999.4 | 7.12 | 7.86 | 12.16 | 12.99 | 82.6 | — | — |
| random:0.01 | primarymaterial | 244 | 5,193 | 18.26 | 12175.3 | 7.12 | 7.79 | 14.56 | 15.86 | 87.7 | — | — |
| random:0.01 | shape | 94 | 5,193 | 14.97 | 17801.9 | 7.12 | 8.06 | 13.77 | 12.49 | 86.6 | — | — |
| random:0.01 | style | 65 | 5,193 | 17.50 | 17350.7 | 7.12 | 8.19 | 16.46 | 16.62 | 74.4 | — | — |
| random:0.03 | color | 2825 | 15,494 | 48.12 | 23712.7 | 7.41 | 7.87 | 15.85 | 15.42 | 172.4 | — | — |
| random:0.03 | material | 162 | 15,494 | 14.05 | 31723.7 | 7.41 | 7.80 | 11.93 | 13.21 | 149.5 | — | — |
| random:0.03 | primarymaterial | 244 | 15,494 | 15.87 | 28814.0 | 7.41 | 8.29 | 12.72 | 13.04 | 139.6 | — | — |
| random:0.03 | shape | 94 | 15,494 | 13.58 | 36602.0 | 7.41 | 8.59 | 12.69 | 12.37 | 128.7 | — | — |
| random:0.03 | style | 65 | 15,494 | 16.58 | 34032.9 | 7.41 | 8.02 | 16.17 | 15.87 | 121.2 | — | — |
| random:0.1 | color | 2825 | 51,649 | 39.37 | 2279.0 | 10.35 | 10.11 | 15.39 | 15.28 | 375.4 | — | — |
| random:0.1 | material | 162 | 51,649 | 13.57 | 1777.4 | 10.35 | 10.69 | 12.79 | 12.82 | 182.3 | — | — |
| random:0.1 | primarymaterial | 244 | 51,649 | 15.11 | 1849.9 | 10.35 | 10.46 | 14.19 | 13.79 | 231.8 | — | — |
| random:0.1 | shape | 94 | 51,649 | 13.55 | 1994.2 | 10.35 | 11.00 | 13.75 | 13.24 | 173.8 | — | — |
| random:0.1 | style | 65 | 51,649 | 14.93 | 1956.7 | 10.35 | 10.69 | 16.57 | 15.19 | 153.3 | — | — |
| random:0.25 | color | 2825 | 129,048 | 25.31 | 2268.3 | 8.46 | 9.26 | 14.55 | 13.72 | 447.0 | — | — |
| random:0.25 | material | 162 | 129,048 | 11.32 | 1883.9 | 8.46 | 8.82 | 11.22 | 11.26 | 246.3 | — | — |
| random:0.25 | primarymaterial | 244 | 129,048 | 12.59 | 1745.1 | 8.46 | 9.71 | 12.24 | 12.25 | 260.6 | — | — |
| random:0.25 | shape | 94 | 129,048 | 11.75 | 2134.5 | 8.46 | 9.35 | 13.61 | 12.30 | 209.5 | — | — |
| random:0.25 | style | 65 | 129,048 | 12.49 | 2037.1 | 8.46 | 8.91 | 12.76 | 13.53 | 182.8 | — | — |
| random:0.5 | color | 2825 | 258,400 | 19.26 | 2289.4 | 8.58 | 9.07 | 13.42 | 13.13 | 457.1 | — | — |
| random:0.5 | material | 162 | 258,400 | 10.59 | 1910.9 | 8.58 | 9.25 | 10.75 | 10.83 | 229.6 | — | — |
| random:0.5 | primarymaterial | 244 | 258,400 | 11.56 | 1816.8 | 8.58 | 8.65 | 11.32 | 11.63 | 267.8 | — | — |
| random:0.5 | shape | 94 | 258,400 | 10.50 | 1995.8 | 8.58 | 9.08 | 11.82 | 11.08 | 205.2 | — | — |
| random:0.5 | style | 65 | 258,400 | 11.88 | 2018.5 | 8.58 | 8.66 | 12.38 | 12.36 | 169.2 | — | — |
| real:facet_disjunctive_multi_dim | color | 2825 | 16,332 | 9.11 | 23612.5 | 7.24 | 7.95 | 8.68 | 8.43 | 0.7 | — | — |
| real:facet_disjunctive_multi_dim | material | 162 | 16,332 | 13.23 | 28432.7 | 7.24 | 8.47 | 13.34 | 11.82 | 32.4 | — | — |
| real:facet_disjunctive_multi_dim | primarymaterial | 244 | 16,332 | 12.57 | 25985.8 | 7.24 | 7.72 | 12.82 | 11.86 | 32.2 | — | — |
| real:facet_disjunctive_multi_dim | shape | 94 | 16,332 | 11.68 | 31731.9 | 7.24 | 7.71 | 12.39 | 11.76 | 23.2 | — | — |
| real:facet_disjunctive_multi_dim | style | 65 | 16,332 | 12.11 | 29575.5 | 7.24 | 7.80 | 12.66 | 11.72 | 24.2 | — | — |
| real:fc3_style_modern_x_color | color | 2825 | 97,188 | 16.68 | 1795.5 | 8.55 | 9.48 | 13.09 | 13.13 | 97.9 | — | — |
| real:fc3_style_modern_x_color | material | 162 | 97,188 | 11.14 | 1611.2 | 8.55 | 9.74 | 11.41 | 11.27 | 78.5 | — | — |
| real:fc3_style_modern_x_color | primarymaterial | 244 | 97,188 | 11.77 | 1585.7 | 8.55 | 9.47 | 12.23 | 11.88 | 55.4 | — | — |
| real:fc3_style_modern_x_color | shape | 94 | 97,188 | 11.92 | 1984.2 | 8.55 | 9.63 | 11.67 | 11.82 | 73.0 | — | — |
| real:fc3_style_modern_x_color | style | 65 | 97,188 | 10.26 | 1766.4 | 8.55 | 10.11 | 11.18 | 9.52 | 2.2 | — | — |
| real:fc4_color_white_x_style | color | 2825 | 17,256 | 9.00 | 25208.4 | 7.15 | 7.94 | 9.28 | 8.42 | 0.7 | — | — |
| real:fc4_color_white_x_style | material | 162 | 17,256 | 13.99 | 30517.8 | 7.15 | 8.21 | 12.71 | 11.80 | 50.9 | — | — |
| real:fc4_color_white_x_style | primarymaterial | 244 | 17,256 | 12.36 | 30382.0 | 7.15 | 7.75 | 12.66 | 11.55 | 32.5 | — | — |
| real:fc4_color_white_x_style | shape | 94 | 17,256 | 11.32 | 33322.0 | 7.15 | 7.78 | 11.67 | 10.83 | 37.5 | — | — |
| real:fc4_color_white_x_style | style | 65 | 17,256 | 13.56 | 33220.1 | 7.15 | 8.92 | 12.54 | 11.79 | 31.8 | — | — |
| real:fc6_depth3_x_shape | color | 2825 | 36 | 104.09 | 2121.5 | 10.56 | 12.72 | 35.20 | 26.55 | 0.6 | — | — |
| real:fc6_depth3_x_shape | material | 162 | 36 | 19.35 | 2083.7 | 10.56 | 11.26 | 12.90 | 12.37 | 0.8 | — | — |
| real:fc6_depth3_x_shape | primarymaterial | 244 | 36 | 26.07 | 1776.6 | 10.56 | 11.20 | 14.67 | 17.73 | 1.0 | — | — |
| real:fc6_depth3_x_shape | shape | 94 | 36 | 20.63 | 2639.0 | 10.56 | 12.71 | 13.73 | 12.73 | 1.8 | — | — |
| real:fc6_depth3_x_shape | style | 65 | 36 | 20.56 | 2186.2 | 10.56 | 11.25 | 12.72 | 12.14 | 2.2 | — | — |
| real:fc7_pergolas_x_color | color | 2825 | 156 | 29.94 | 3237.7 | 8.07 | 8.85 | 14.78 | 12.18 | 0.6 | — | — |
| real:fc7_pergolas_x_color | material | 162 | 156 | 10.83 | 2668.3 | 8.07 | 8.65 | 9.45 | 9.45 | 0.8 | — | — |
| real:fc7_pergolas_x_color | primarymaterial | 244 | 156 | 11.03 | 2345.3 | 8.07 | 8.79 | 9.54 | 9.59 | 0.8 | — | — |
| real:fc7_pergolas_x_color | shape | 94 | 156 | 12.25 | 3272.3 | 8.07 | 9.82 | 10.01 | 9.69 | 2.3 | — | — |
| real:fc7_pergolas_x_color | style | 65 | 156 | 11.28 | 2605.0 | 8.07 | 8.73 | 9.51 | 9.00 | 3.1 | — | — |
| real:numeric_range_sort | color | 2825 | 383,604 | 16.20 | 2585.8 | 7.88 | 8.58 | 12.25 | 12.27 | 365.0 | — | — |
| real:numeric_range_sort | material | 162 | 383,604 | 10.54 | 1874.4 | 7.88 | 8.47 | 11.17 | 10.63 | 209.7 | — | — |
| real:numeric_range_sort | primarymaterial | 244 | 383,604 | 10.66 | 1802.8 | 7.88 | 8.69 | 13.28 | 10.78 | 227.4 | — | — |
| real:numeric_range_sort | shape | 94 | 383,604 | 10.56 | 2131.6 | 7.88 | 8.71 | 11.02 | 13.31 | 191.6 | — | — |
| real:numeric_range_sort | style | 65 | 383,604 | 11.28 | 1947.1 | 7.88 | 9.01 | 11.42 | 11.85 | 161.0 | — | — |
| real:sc3_accent_chairs_review_count_desc | color | 2825 | 13,236 | 7.81 | 19838.7 | 7.15 | 8.01 | 9.02 | 7.83 | 0.6 | — | — |
| real:sc3_accent_chairs_review_count_desc | material | 162 | 13,236 | 7.41 | 22872.3 | 7.15 | 8.16 | 7.76 | 8.44 | 0.8 | — | — |
| real:sc3_accent_chairs_review_count_desc | primarymaterial | 244 | 13,236 | 7.85 | 22338.8 | 7.15 | 8.01 | 8.08 | 8.66 | 0.7 | — | — |
| real:sc3_accent_chairs_review_count_desc | shape | 94 | 13,236 | 7.66 | 26761.3 | 7.15 | 8.50 | 8.90 | 8.48 | 0.9 | — | — |
| real:sc3_accent_chairs_review_count_desc | style | 65 | 13,236 | 13.35 | 24324.6 | 7.15 | 8.44 | 13.70 | 12.44 | 18.1 | — | — |
| real:sc6_depth5_review_count_desc | color | 2825 | 12 | 283.60 | 1827.1 | 16.07 | 16.96 | 83.51 | 59.89 | 0.7 | — | — |
| real:sc6_depth5_review_count_desc | material | 162 | 12 | 47.14 | 1706.6 | 16.07 | 16.27 | 24.56 | 21.86 | 0.8 | — | — |
| real:sc6_depth5_review_count_desc | primarymaterial | 244 | 12 | 61.84 | 1402.0 | 16.07 | 16.85 | 27.42 | 25.84 | 1.1 | — | — |
| real:sc6_depth5_review_count_desc | shape | 94 | 12 | 49.71 | 1959.0 | 16.07 | 15.68 | 24.16 | 22.46 | 1.7 | — | — |
| real:sc6_depth5_review_count_desc | style | 65 | 12 | 45.29 | 1608.1 | 16.07 | 16.46 | 23.35 | 22.76 | 2.1 | — | — |

