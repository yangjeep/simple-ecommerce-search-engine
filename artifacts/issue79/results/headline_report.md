## Headline cells: CPU/query us (median of 3 runs; CV) and P95 ms

### facet_low_cardinality_style
fastest same-host competitor: meilisearch 9948us; all: meilisearch=9948, solr=48123, typesense=268560
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 170669 | 0.186 | 164.84 | 223.80 | - | - | - | - | - | - | 1.00x | 17.2 |
| bitmap:hybrid | 8647 | 0.079 | 8.68 | 13.60 | 7649 | 578 | 117 | 8346 | 48 | bitmap | 19.74x | 0.869 |
| bitmap:legacy | 192100 | 0.016 | 197.42 | 230.36 | 6461 | 526 | 184777 | 191767 | 515928 | bitmap | 0.89x | 19.3 |
| hybrid:hybrid | 8538 | 0.063 | 8.79 | 12.83 | 7521 | 571 | 117 | 8211 | 48 | bitmap | 19.99x | 0.858 |
| hybrid:legacy | 185975 | 0.014 | 176.76 | 227.57 | 6334 | 506 | 179093 | 185754 | 515928 | bitmap | 0.92x | 18.7 |
| legacy:hybrid | 16179 | 0.028 | 16.31 | 21.00 | 7435 | 8209 | 118 | 15762 | 48 | legacy | 10.55x | 1.63 |
| legacy:legacy | 195886 | 0.041 | 194.55 | 236.39 | 6460 | 7164 | 182106 | 195582 | 515928 | legacy | 0.87x | 19.7 |
| ordinal:hybrid | 15499 | 0.061 | 15.75 | 18.29 | 7343 | 7693 | 121 | 15159 | 48 | ordinal | 11.01x | 1.56 |
| ordinal:legacy | 196620 | 0.027 | 201.03 | 239.42 | 6574 | 7115 | 182575 | 196312 | 515928 | ordinal | 0.87x | 19.8 |

### facet_medium_cardinality_primarymaterial
fastest same-host competitor: meilisearch 9326us; all: meilisearch=9326, solr=42470, typesense=271582
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 174457 | 0.160 | 169.97 | 218.37 | - | - | - | - | - | - | 1.00x | 18.7 |
| bitmap:hybrid | 11091 | 0.057 | 10.12 | 19.75 | 8957 | 1453 | 123 | 10694 | 48 | bitmap | 15.73x | 1.19 |
| bitmap:legacy | 184552 | 0.123 | 189.40 | 219.48 | 6383 | 1275 | 176525 | 184186 | 515928 | bitmap | 0.95x | 19.8 |
| hybrid:hybrid | 11186 | 0.018 | 10.11 | 20.60 | 9087 | 1539 | 125 | 10754 | 48 | bitmap | 15.60x | 1.2 |
| hybrid:legacy | 180097 | 0.164 | 174.44 | 233.48 | 6366 | 1245 | 172146 | 179759 | 515928 | bitmap | 0.97x | 19.3 |
| legacy:hybrid | 19065 | 0.114 | 18.36 | 32.27 | 8235 | 10129 | 122 | 18488 | 48 | legacy | 9.15x | 2.04 |
| legacy:legacy | 193980 | 0.034 | 191.63 | 234.11 | 6489 | 8586 | 178718 | 193631 | 515928 | legacy | 0.90x | 20.8 |
| ordinal:hybrid | 18525 | 0.056 | 17.53 | 34.36 | 8881 | 8872 | 135 | 17847 | 48 | ordinal | 9.42x | 1.99 |
| ordinal:legacy | 189305 | 0.122 | 189.30 | 228.06 | 6377 | 6866 | 175753 | 188999 | 515928 | ordinal | 0.92x | 20.3 |

### facet_high_cardinality_color
fastest same-host competitor: meilisearch 7987us; all: meilisearch=7987, solr=43654, typesense=267879
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 188889 | 0.176 | 181.96 | 252.75 | - | - | - | - | - | - | 1.00x | 23.7 |
| bitmap:hybrid | 25458 | 0.099 | 24.05 | 42.99 | 9829 | 12736 | 133 | 23663 | 48 | bitmap | 7.42x | 3.19 |
| bitmap:legacy | 182892 | 0.164 | 173.12 | 268.70 | 7131 | 9159 | 165588 | 181883 | 515928 | bitmap | 1.03x | 22.9 |
| hybrid:hybrid | 22868 | 0.087 | 20.86 | 34.89 | 8683 | 12332 | 126 | 21138 | 48 | ordinal | 8.26x | 2.86 |
| hybrid:legacy | 194407 | 0.112 | 198.79 | 275.95 | 7345 | 11490 | 177031 | 193367 | 515928 | ordinal | 0.97x | 24.3 |
| legacy:hybrid | 41554 | 0.141 | 38.57 | 66.50 | 9563 | 30255 | 128 | 39948 | 48 | legacy | 4.55x | 5.2 |
| legacy:legacy | 199720 | 0.217 | 198.63 | 247.22 | 6628 | 23082 | 168982 | 198694 | 515928 | legacy | 0.95x | 25 |
| ordinal:hybrid | 24677 | 0.196 | 24.12 | 37.39 | 9535 | 13107 | 148 | 22819 | 48 | ordinal | 7.65x | 3.09 |
| ordinal:legacy | 187487 | 0.167 | 174.67 | 262.59 | 7149 | 11322 | 168051 | 186524 | 515928 | ordinal | 1.01x | 23.5 |

### facet_disjunctive_multi_dim
fastest same-host competitor: meilisearch 20206us; all: meilisearch=20206, solr=47505, typesense=313822
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 57773 | 0.104 | 48.85 | 98.63 | - | - | - | - | - | - | 1.00x | 2.86 |
| bitmap:hybrid | 45508 | 0.027 | 46.75 | 65.93 | 29 | 44159 | 115 | 44304 | 48 | bitmap,bitmap,bitmap,bitmap,bitmap | 1.27x | 2.25 |
| bitmap:legacy | 58638 | 0.048 | 56.46 | 91.00 | 28 | 47642 | 9626 | 57298 | 16332 | bitmap,bitmap,bitmap,bitmap,bitmap | 0.99x | 2.9 |
| hybrid:hybrid | 25857 | 0.195 | 23.42 | 47.53 | 30 | 24301 | 122 | 24455 | 48 | ordinal,ordinal,ordinal,ordinal,ordinal | 2.23x | 1.28 |
| hybrid:legacy | 34414 | 0.059 | 32.65 | 55.69 | 28 | 23267 | 9896 | 33193 | 16332 | ordinal,ordinal,ordinal,ordinal,ordinal | 1.68x | 1.7 |
| legacy:hybrid | 61890 | 0.014 | 62.73 | 80.92 | 27 | 60503 | 118 | 60650 | 48 | legacy,legacy,legacy,legacy,legacy | 0.93x | 3.06 |
| legacy:legacy | 71770 | 0.070 | 70.77 | 103.61 | 28 | 61094 | 9400 | 70523 | 16332 | legacy,legacy,legacy,legacy,legacy | 0.80x | 3.55 |
| ordinal:hybrid | 24493 | 0.102 | 23.14 | 40.21 | 29 | 23042 | 120 | 23194 | 48 | ordinal,ordinal,ordinal,ordinal,ordinal | 2.36x | 1.21 |
| ordinal:legacy | 38220 | 0.096 | 33.97 | 65.23 | 29 | 25316 | 10565 | 36573 | 16332 | ordinal,ordinal,ordinal,ordinal,ordinal | 1.51x | 1.89 |

### numeric_range_sort
fastest same-host competitor: meilisearch 9164us; all: meilisearch=9164, solr=9432, typesense=318875
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 666125 | 0.194 | 635.68 | 820.75 | - | - | - | - | - | - | 1.00x | 72.7 |
| hybrid:hybrid | 7832 | 0.071 | 7.40 | 14.53 | 7292 | 1 | 146 | 7438 | 48 | presorted | 85.06x | 0.855 |
| legacy:hybrid | 7038 | 0.077 | 7.11 | 11.12 | 6515 | 1 | 141 | 6664 | 48 | presorted | 94.64x | 0.768 |
| legacy:legacy | 872256 | 0.035 | 856.46 | 1083.44 | 5279 | 1 | 866799 | 872081 | 383604 | legacy_full | 0.76x | 95.2 |
| legacy:presorted | 7217 | 0.124 | 7.21 | 10.43 | 6639 | 1 | 148 | 6790 | 48 | presorted | 92.30x | 0.788 |
| legacy:topk | 16718 | 0.022 | 17.34 | 24.89 | 5997 | 1 | 10478 | 16397 | 383604 | topk | 39.85x | 1.82 |

### sh2_color_white_review_count_desc
no competitor data (native-only cell)
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 43474 | 0.174 | 39.29 | 63.90 | - | - | - | - | - | - | 1.00x | nan |
| hybrid:hybrid | 646 | 0.096 | 0.76 | 2.05 | 23 | 0 | 269 | 293 | 852 | presorted | 67.33x | nan |
| legacy:hybrid | 538 | 0.139 | 0.68 | 1.40 | 18 | 0 | 246 | 265 | 852 | presorted | 80.85x | nan |
| legacy:legacy | 52159 | 0.064 | 47.72 | 73.23 | 28 | 1 | 51745 | 51777 | 17256 | legacy_full | 0.83x | nan |
| legacy:presorted | 593 | 0.181 | 0.65 | 1.56 | 22 | 0 | 252 | 276 | 852 | presorted | 73.27x | nan |
| legacy:topk | 1773 | 0.105 | 1.98 | 3.75 | 27 | 1 | 1370 | 1399 | 17256 | topk | 24.53x | nan |

### sh3_depth3_review_count_desc
no competitor data (native-only cell)
| variant | cpu us | CV | p50 ms | p95 ms | cand us | facets us | sort us | server total us | inspected | path | N0/variant | vs fastest |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| N0 (unchanged #77 binary) | 607 | 0.017 | 0.80 | 1.27 | - | - | - | - | - | - | 1.00x | nan |
| hybrid:hybrid | 653 | 0.151 | 0.84 | 1.79 | 153 | 1 | 115 | 270 | 36 | topk | 0.93x | nan |
| legacy:hybrid | 506 | 0.118 | 0.61 | 1.59 | 117 | 0 | 96 | 216 | 36 | topk | 1.20x | nan |
| legacy:legacy | 707 | 0.156 | 0.83 | 1.85 | 135 | 1 | 259 | 395 | 36 | legacy_full | 0.86x | nan |
| legacy:presorted | 15744 | 0.112 | 14.04 | 19.71 | 128 