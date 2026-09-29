# Issue #64 — facet economics (500k, CPU/query µs, median of 3 runs)

**Verdict over the realistic grid R (32 cells): KEEP** — classes {'MATERIAL': 32}; NOT_EQUIVALENT_WORK (excluded): 0

| cell | scope size | k | native | meili | solr | fastest | r | class | robust | native cores/1k QPS | fastest cores/1k QPS |
|---|---|---|---|---|---|---|---|---|---|---|---|
| i64_s0_k0 (ref) | 515,928 | 0 | 438 | 6,308 | 20,134 | meilisearch | — | baseline | — | 0.44 | 6.31 |
| i64_s0_k1 (ref) | 515,928 | 1 | 1,078 | 10,324 | 57,521 | meilisearch | 0.104 | MATERIAL | robust | 1.08 | 10.32 |
| i64_s0_k3 (ref) | 515,928 | 3 | 25,274 | 67,064 | 145,064 | meilisearch | 0.377 | MATERIAL | robust | 25.27 | 67.06 |
| i64_s0_k5 (ref) | 515,928 | 5 | 27,197 | 79,563 | 164,785 | meilisearch | 0.342 | MATERIAL | robust | 27.20 | 79.56 |
| i64_s0_k8 (ref) | 515,928 | 8 | 36,801 | 127,107 | 227,403 | meilisearch | 0.290 | MATERIAL | robust | 36.80 | 127.11 |
| i64_s0_k11 (ref) | 515,928 | 11 | 59,871 | 150,217 | 293,728 | meilisearch | 0.399 | MATERIAL | robust | 59.87 | 150.22 |
| i64_s0_color (ref) | 515,928 | color | 21,927 | 56,258 | 83,712 | meilisearch | 0.390 | MATERIAL | robust | 21.93 | 56.26 |
| i64_s1_k0 (ref) | 192,468 | 0 | 422 | 6,048 | 9,047 | meilisearch | — | baseline | — | 0.42 | 6.05 |
| i64_s1_k1 | 192,468 | 1 | 1,192 | 10,553 | 26,959 | meilisearch | 0.113 | MATERIAL | robust | 1.19 | 10.55 |
| i64_s1_k3 | 192,468 | 3 | 12,760 | 38,166 | 62,600 | meilisearch | 0.334 | MATERIAL | robust | 12.76 | 38.17 |
| i64_s1_k5 | 192,468 | 5 | 13,248 | 45,836 | 83,217 | meilisearch | 0.289 | MATERIAL | robust | 13.25 | 45.84 |
| i64_s1_k8 | 192,468 | 8 | 28,666 | 65,699 | 125,374 | meilisearch | 0.436 | MATERIAL | robust | 28.67 | 65.70 |
| i64_s1_k10 | 192,468 | 10 | 31,733 | 69,288 | 129,543 | meilisearch | 0.458 | MATERIAL | robust | 31.73 | 69.29 |
| i64_s1_color | 192,468 | color | 8,439 | 31,629 | 32,778 | meilisearch | 0.267 | MATERIAL | robust | 8.44 | 31.63 |
| i64_s2_k0 (ref) | 55,344 | 0 | 406 | 6,137 | 7,686 | meilisearch | — | baseline | — | 0.41 | 6.14 |
| i64_s2_k1 | 55,344 | 1 | 2,752 | 10,592 | 14,235 | meilisearch | 0.260 | MATERIAL | robust | 2.75 | 10.59 |
| i64_s2_k3 | 55,344 | 3 | 9,634 | 40,972 | 37,272 | solr | 0.258 | MATERIAL | robust | 9.63 | 37.27 |
| i64_s2_k5 | 55,344 | 5 | 14,864 | 47,568 | 44,119 | solr | 0.337 | MATERIAL | robust | 14.86 | 44.12 |
| i64_s2_k8 | 55,344 | 8 | 18,247 | 57,628 | 67,752 | meilisearch | 0.317 | MATERIAL | robust | 18.25 | 57.63 |
| i64_s2_k10 | 55,344 | 10 | 22,639 | 61,818 | 72,000 | meilisearch | 0.366 | MATERIAL | robust | 22.64 | 61.82 |
| i64_s2_color | 55,344 | color | 4,719 | 32,786 | 24,746 | solr | 0.191 | MATERIAL | robust | 4.72 | 24.75 |
| i64_s2_k5_style1 | 55,344 | style1 | 6,865 | 40,387 | 24,513 | solr | 0.280 | MATERIAL | robust | 6.87 | 24.51 |
| i64_s2_k5_style2 | 55,344 | style2 | 8,426 | 45,954 | 34,022 | solr | 0.248 | MATERIAL | robust | 8.43 | 34.02 |
| i64_s3_k0 (ref) | 14,472 | 0 | 402 | 6,587 | 7,880 | meilisearch | — | baseline | — | 0.40 | 6.59 |
| i64_s3_k1 | 14,472 | 1 | 814 | 8,082 | 12,276 | meilisearch | 0.101 | MATERIAL | robust | 0.81 | 8.08 |
| i64_s3_k3 | 14,472 | 3 | 2,619 | 27,889 | 19,186 | solr | 0.137 | MATERIAL | robust | 2.62 | 19.19 |
| i64_s3_k5 | 14,472 | 5 | 4,286 | 34,539 | 18,551 | solr | 0.231 | MATERIAL | robust | 4.29 | 18.55 |
| i64_s3_k8 | 14,472 | 8 | 6,216 | 43,434 | 28,955 | solr | 0.215 | MATERIAL | robust | 6.22 | 28.95 |
| i64_s3_k9 | 14,472 | 9 | 6,690 | 42,617 | 35,248 | solr | 0.190 | MATERIAL | robust | 6.69 | 35.25 |
| i64_s3_color | 14,472 | color | 1,662 | 24,360 | 14,296 | solr | 0.116 | MATERIAL | robust | 1.66 | 14.30 |
| i64_s3_k5_style1 | 14,472 | style1 | 1,151 | 15,760 | 10,803 | solr | 0.107 | MATERIAL | robust | 1.15 | 10.80 |
| i64_s3_k5_style2 | 14,472 | style2 | 1,293 | 20,475 | 11,320 | solr | 0.114 | MATERIAL | robust | 1.29 | 11.32 |
| i64_s4_k0 (ref) | 3,024 | 0 | 380 | 6,127 | 5,508 | solr | — | baseline | — | 0.38 | 5.51 |
| i64_s4_k1 | 3,024 | 1 | 604 | 7,286 | 9,971 | meilisearch | 0.083 | MATERIAL | robust | 0.60 | 7.29 |
| i64_s4_k3 | 3,024 | 3 | 1,149 | 17,080 | 12,033 | solr | 0.096 | MATERIAL | robust | 1.15 | 12.03 |
| i64_s4_k5 | 3,024 | 5 | 1,479 | 18,436 | 9,346 | solr | 0.158 | MATERIAL | robust | 1.48 | 9.35 |
| i64_s4_k8 | 3,024 | 8 | 1,954 | 20,919 | 11,771 | solr | 0.166 | MATERIAL | robust | 1.95 | 11.77 |
| i64_s4_color | 3,024 | color | 733 | 13,800 | 8,628 | solr | 0.085 | MATERIAL | robust | 0.73 | 8.63 |
| i64_s5_k0 (ref) | 600 | 0 | 370 | 5,977 | 7,203 | meilisearch | — | baseline | — | 0.37 | 5.98 |
| i64_s5_k1 | 600 | 1 | 457 | 9,200 | 6,089 | solr | 0.075 | MATERIAL | robust | 0.46 | 6.09 |
| i64_s5_k3 | 600 | 3 | 509 | 14,659 | 7,466 | solr | 0.068 | MATERIAL | robust | 0.51 | 7.47 |
| i64_s5_k5 | 600 | 5 | 699 | 19,952 | 5,810 | solr | 0.120 | MATERIAL | robust | 0.70 | 5.81 |
| i64_s5_k8 | 600 | 8 | 845 | 25,953 | 7,945 | solr | 0.106 | MATERIAL | robust | 0.84 | 7.95 |
| i64_s5_color | 600 | color | 472 | 8,613 | 6,780 | solr | 0.070 | MATERIAL | robust | 0.47 | 6.78 |

⚠NEW = NOT_EQUIVALENT_WORK (missing or non-equivalent dump) in at least one run.

## Cost curves: incremental CPU per added facet (least-squares slope over the ladder, µs/facet) and k=0 baseline

| scope | size | native k0 | native slope | meili k0 | meili slope | solr k0 | solr slope |
|---|---|---|---|---|---|---|---|
| s0 | 515,928 | 438 | 5,137 | 6,308 | 13,396 | 20,134 | 22,014 |
| s1 | 192,468 | 422 | 3,371 | 6,048 | 6,263 | 9,047 | 11,649 |
| s2 | 55,344 | 406 | 2,075 | 6,137 | 5,079 | 7,686 | 6,346 |
| s3 | 14,472 | 402 | 732 | 6,587 | 4,048 | 7,880 | 2,624 |
| s4 | 3,024 | 380 | 188 | 6,127 | 1,766 | 5,508 | 138 |
| s5 | 600 | 370 | 59 | 5,977 | 2,395 | 7,203 | 188 |

## Breakpoints (class by scope × k, ladder cells)

| scope | k=1 | k=3 | k=5 | k=8 | k=9 | k=10 | k=11 | color |
|---|---|---|---|---|---|---|---|---|
| s0 (515,928) | MATE 0.10 | MATE 0.38 | MATE 0.34 | MATE 0.29 |  |  | MATE 0.40 | MATE 0.39 |
| s1 (192,468) | MATE 0.11 | MATE 0.33 | MATE 0.29 | MATE 0.44 |  | MATE 0.46 |  | MATE 0.27 |
| s2 (55,344) | MATE 0.26 | MATE 0.26 | MATE 0.34 | MATE 0.32 |  | MATE 0.37 |  | MATE 0.19 |
| s3 (14,472) | MATE 0.10 | MATE 0.14 | MATE 0.23 | MATE 0.21 | MATE 0.19 |  |  | MATE 0.12 |
| s4 (3,024) | MATE 0.08 | MATE 0.10 | MATE 0.16 | MATE 0.17 |  |  |  | MATE 0.08 |
| s5 (600) | MATE 0.08 | MATE 0.07 | MATE 0.12 | MATE 0.11 |  |  |  | MATE 0.07 |

## Latency (P50 / P95 / P99 ms, medians over runs) and backend requests

| cell | native | meili | solr |
|---|---|---|---|
| i64_s0_k0 | 0.6 / 1.0 / 1.3 (1 req) | 6.5 / 9.1 / 13.3 (1 req) | 7.9 / 12.5 / 15.0 (1 req) |
| i64_s0_k1 | 1.4 / 1.9 / 2.2 (1 req) | 10.7 / 14.2 / 15.2 (1 req) | 46.2 / 56.6 / 77.0 (1 req) |
| i64_s0_k3 | 25.4 / 31.8 / 35.8 (1 req) | 80.1 / 95.2 / 120.1 (1 req) | 174.6 / 211.4 / 236.3 (1 req) |
| i64_s0_k5 | 27.7 / 36.4 / 41.1 (1 req) | 93.5 / 114.9 / 133.2 (1 req) | 171.3 / 266.7 / 300.3 (1 req) |
| i64_s0_k8 | 36.4 / 50.9 / 79.9 (1 req) | 150.6 / 173.8 / 191.5 (1 req) | 242.3 / 349.0 / 412.0 (1 req) |
| i64_s0_k11 | 59.9 / 73.8 / 109.7 (1 req) | 205.4 / 243.3 / 279.1 (1 req) | 338.5 / 439.4 / 489.3 (1 req) |
| i64_s0_color | 22.5 / 29.8 / 33.7 (1 req) | 68.1 / 86.7 / 97.8 (1 req) | 99.9 / 122.6 / 136.0 (1 req) |
| i64_s1_k0 | 0.6 / 1.0 / 1.2 (1 req) | 6.6 / 8.2 / 9.3 (1 req) | 4.8 / 6.9 / 7.5 (1 req) |
| i64_s1_k1 | 1.4 / 2.2 / 2.7 (1 req) | 11.0 / 14.2 / 16.3 (1 req) | 22.6 / 30.4 / 33.4 (1 req) |
| i64_s1_k3 | 13.8 / 20.4 / 33.0 (1 req) | 40.9 / 51.1 / 63.8 (1 req) | 61.6 / 75.7 / 88.2 (1 req) |
| i64_s1_k5 | 14.8 / 20.1 / 22.7 (1 req) | 49.2 / 60.1 / 69.7 (1 req) | 84.9 / 102.4 / 114.5 (1 req) |
| i64_s1_k8 | 29.7 / 51.4 / 64.7 (1 req) | 71.7 / 86.8 / 105.1 (1 req) | 145.6 / 172.0 / 202.5 (1 req) |
| i64_s1_k10 | 35.5 / 43.7 / 48.8 (1 req) | 77.5 / 93.2 / 103.1 (1 req) | 130.0 / 196.0 / 204.8 (1 req) |
| i64_s1_color | 8.8 / 10.2 / 12.6 (1 req) | 34.8 / 42.0 / 48.8 (1 req) | 35.6 / 46.0 / 50.3 (1 req) |
| i64_s2_k0 | 0.6 / 1.0 / 1.3 (1 req) | 6.8 / 9.0 / 10.0 (1 req) | 4.3 / 5.7 / 6.6 (1 req) |
| i64_s2_k1 | 3.1 / 4.2 / 4.9 (1 req) | 10.9 / 15.6 / 19.8 (1 req) | 11.2 / 15.0 / 17.4 (1 req) |
| i64_s2_k3 | 9.9 / 13.0 / 16.3 (1 req) | 45.1 / 57.1 / 62.2 (1 req) | 41.9 / 54.0 / 60.4 (1 req) |
| i64_s2_k5 | 14.5 / 18.2 / 20.5 (1 req) | 52.0 / 63.5 / 74.5 (1 req) | 53.0 / 65.4 / 73.2 (1 req) |
| i64_s2_k8 | 19.8 / 25.6 / 30.1 (1 req) | 65.8 / 76.0 / 86.1 (1 req) | 76.0 / 97.7 / 104.1 (1 req) |
| i64_s2_k10 | 23.2 / 30.6 / 35.0 (1 req) | 67.5 / 81.7 / 95.8 (1 req) | 82.0 / 101.2 / 108.1 (1 req) |
| i64_s2_color | 5.2 / 6.7 / 7.3 (1 req) | 37.3 / 44.9 / 51.6 (1 req) | 30.3 / 41.0 / 46.6 (1 req) |
| i64_s2_k5_style1 | 7.2 / 8.4 / 10.1 (1 req) | 42.6 / 52.7 / 58.1 (2 req) | 25.5 / 33.2 / 42.5 (1 req) |
| i64_s2_k5_style2 | 8.3 / 10.7 / 16.6 (1 req) | 48.8 / 59.2 / 67.8 (2 req) | 30.1 / 39.5 / 49.4 (1 req) |
| i64_s3_k0 | 0.6 / 0.9 / 1.1 (1 req) | 6.9 / 9.4 / 10.0 (1 req) | 3.8 / 5.3 / 6.4 (1 req) |
| i64_s3_k1 | 1.1 / 1.4 / 1.8 (1 req) | 8.4 / 11.3 / 12.5 (1 req) | 6.5 / 8.5 / 11.4 (1 req) |
| i64_s3_k3 | 3.0 / 4.1 / 4.9 (1 req) | 29.2 / 38.6 / 43.0 (1 req) | 17.3 / 24.2 / 25.1 (1 req) |
| i64_s3_k5 | 4.6 / 6.2 / 7.8 (1 req) | 36.2 / 44.6 / 50.6 (1 req) | 21.4 / 27.5 / 31.5 (1 req) |
| i64_s3_k8 | 6.5 / 8.1 / 9.0 (1 req) | 44.8 / 57.9 / 65.2 (1 req) | 27.9 / 39.4 / 44.4 (1 req) |
| i64_s3_k9 | 6.9 / 8.9 / 14.0 (1 req) | 45.0 / 55.6 / 62.6 (1 req) | 28.4 / 37.8 / 41.3 (1 req) |
| i64_s3_color | 2.0 / 2.6 / 3.3 (1 req) | 25.3 / 33.3 / 40.1 (1 req) | 13.9 / 18.4 / 23.4 (1 req) |
| i64_s3_k5_style1 | 1.5 / 2.0 / 2.4 (1 req) | 16.3 / 20.8 / 22.3 (2 req) | 7.4 / 9.9 / 10.3 (1 req) |
| i64_s3_k5_style2 | 1.6 / 2.2 / 2.6 (1 req) | 20.6 / 28.1 / 32.1 (2 req) | 7.6 / 10.0 / 11.3 (1 req) |
| i64_s4_k0 | 0.7 / 0.9 / 1.1 (1 req) | 6.6 / 9.5 / 12.5 (1 req) | 3.5 / 5.0 / 7.9 (1 req) |
| i64_s4_k1 | 0.8 / 1.2 / 1.5 (1 req) | 7.8 / 9.9 / 10.6 (1 req) | 5.4 / 7.2 / 8.8 (1 req) |
| i64_s4_k3 | 1.5 / 2.0 / 2.4 (1 req) | 17.5 / 22.8 / 24.3 (1 req) | 8.6 / 11.6 / 15.8 (1 req) |
| i64_s4_k5 | 1.8 / 2.4 / 2.9 (1 req) | 18.8 / 23.9 / 25.8 (1 req) | 9.7 / 12.6 / 14.5 (1 req) |
| i64_s4_k8 | 2.3 / 3.1 / 4.1 (1 req) | 21.5 / 25.7 / 30.1 (1 req) | 11.5 / 14.8 / 15.8 (1 req) |
| i64_s4_color | 1.0 / 1.4 / 1.9 (1 req) | 14.8 / 20.3 / 22.2 (1 req) | 7.5 / 9.9 / 14.0 (1 req) |
| i64_s5_k0 | 0.6 / 0.9 / 1.1 (1 req) | 6.6 / 8.8 / 9.5 (1 req) | 3.9 / 4.9 / 5.5 (1 req) |
| i64_s5_k1 | 0.7 / 1.1 / 1.4 (1 req) | 9.5 / 13.4 / 14.5 (1 req) | 4.8 / 6.2 / 6.8 (1 req) |
| i64_s5_k3 | 0.7 / 1.1 / 1.3 (1 req) | 14.6 / 19.4 / 21.8 (1 req) | 5.2 / 6.8 / 7.4 (1 req) |
| i64_s5_k5 | 1.0 / 1.4 / 1.7 (1 req) | 19.7 / 27.3 / 29.7 (1 req) | 5.6 / 7.4 / 8.1 (1 req) |
| i64_s5_k8 | 1.1 / 1.6 / 2.3 (1 req) | 26.3 / 29.9 / 32.3 (1 req) | 6.4 / 8.3 / 9.5 (1 req) |
| i64_s5_color | 0.7 / 1.2 / 2.0 (1 req) | 9.2 / 11.0 / 12.5 (1 req) | 3.8 / 5.0 / 6.1 (1 req) |

RSS (median over runs, bytes): native 4,411,047,936, meilisearch 5,549,580,288, solr 3,849,076,736

Run status:

- run 1 native: status=ok correctness=True
- run 1 meilisearch: status=ok correctness=True
- run 1 solr: status=ok correctness=True
- run 2 native: status=ok correctness=True
- run 2 meilisearch: status=ok correctness=True
- run 2 solr: status=ok correctness=True
- run 3 native: status=ok correctness=True
- run 3 meilisearch: status=ok correctness=True
- run 3 solr: status=ok correctness=True
