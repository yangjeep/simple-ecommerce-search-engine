# Issue #63 Part A — same-window equal-work confirmation (500k, CPU/query µs)

N+ = `p0r`

| cell | native FINAL (runs) | N+ | meili | solr | fastest | native r | class | robust? | N+ r | N+ class |
|---|---|---|---|---|---|---|---|---|---|---|
| FH1 | 8,530 (7,057 / 9,873 / 8,530) | 1,077 | 10,798 | 48,978 | meilisearch | 0.790 | PARITY | median-only | 0.100 | MATERIAL FACET ADVANTAGE |
| FH2 | 12,470 (12,470 / 14,084 / 12,409) | 2,019 | 14,038 | 50,948 | meilisearch | 0.888 | PARITY | robust | 0.144 | MATERIAL FACET ADVANTAGE |
| FH3 | 37,219 (35,947 / 37,219 / 37,942) | 22,550 | 53,050 | 93,560 | meilisearch | 0.702 | MATERIAL FACET ADVANTAGE | robust | 0.425 | MATERIAL FACET ADVANTAGE |
| FH4 | 37,394 (32,836 / 42,609 / 37,394) | 25,223 | 72,899 | 93,353 | meilisearch | 0.513 | MATERIAL FACET ADVANTAGE | robust | 0.346 | MATERIAL FACET ADVANTAGE |
| SH1 | 7,371 (6,564 / 7,473 / 7,371) | 8,169 | 6,403 | 9,824 | meilisearch | 1.151 | PARITY | median-only | 1.276 | FACET DISADVANTAGE |
| FD1 | 396 (396 / 556 / 370) | 401 | 5,744 | 22,230 | meilisearch | 0.069 | MATERIAL FACET ADVANTAGE | robust | 0.070 | MATERIAL FACET ADVANTAGE |
| FD3 | 524 (543 / 524 / 516) | 506 | 5,366 | 14,338 | meilisearch | 0.098 | MATERIAL FACET ADVANTAGE | robust | 0.094 | MATERIAL FACET ADVANTAGE |
| FD5 | 522 (530 / 409 / 522) | 506 | 3,628 | 11,400 | meilisearch | 0.144 | MATERIAL FACET ADVANTAGE | robust | 0.139 | MATERIAL FACET ADVANTAGE |

⚠NEW = NOT_EQUIVALENT_WORK in at least one run (excluded from `fastest`).

Latency and variance (medians over runs):

| cell | arm | P50 ms | P95 ms | P99 ms | mean wall ms | backend requests | CV | min | max |
|---|---|---|---|---|---|---|---|---|---|
| FH1 | native | 5.68 | 18.35 | 20.63 | 8.96 | 1 | 0.136 | 7,057 | 9,873 |
| FH1 | native+ | 1.30 | 1.88 | 2.56 | 1.36 | 1 | 0.053 | 1,059 | 1,192 |
| FH1 | meilisearch | 10.99 | 14.64 | 22.21 | 11.68 | 1 | 0.055 | 10,334 | 11,783 |
| FH1 | solr | 42.25 | 53.56 | 65.80 | 37.85 | 1 | 0.058 | 48,072 | 54,682 |
| FH2 | native | 14.74 | 19.07 | 22.02 | 13.00 | 1 | 0.060 | 12,409 | 14,084 |
| FH2 | native+ | 2.23 | 3.44 | 4.34 | 2.35 | 1 | 0.014 | 1,973 | 2,042 |
| FH2 | meilisearch | 15.12 | 18.49 | 21.26 | 15.59 | 1 | 0.002 | 13,981 | 14,039 |
| FH2 | solr | 43.63 | 59.71 | 109.68 | 44.79 | 1 | 0.051 | 47,461 | 53,738 |
| FH3 | native | 36.81 | 50.11 | 78.46 | 38.03 | 1 | 0.022 | 35,947 | 37,942 |
| FH3 | native+ | 23.78 | 35.17 | 47.29 | 23.39 | 1 | 0.077 | 19,734 | 23,788 |
| FH3 | meilisearch | 66.95 | 81.66 | 96.39 | 66.00 | 1 | 0.047 | 52,085 | 57,944 |
| FH3 | solr | 104.01 | 136.97 | 212.79 | 105.03 | 1 | 0.076 | 80,824 | 96,824 |
| FH4 | native | 39.41 | 49.90 | 64.31 | 38.08 | 1 | 0.106 | 32,836 | 42,609 |
| FH4 | native+ | 27.28 | 33.98 | 53.48 | 25.94 | 1 | 0.031 | 24,322 | 26,226 |
| FH4 | meilisearch | 86.27 | 102.61 | 115.73 | 87.10 | 2 | 0.012 | 72,518 | 74,543 |
| FH4 | solr | 114.96 | 137.47 | 174.02 | 113.49 | 1 | 0.021 | 90,235 | 94,853 |
| SH1 | native | 5.79 | 13.43 | 17.61 | 7.79 | 1 | 0.057 | 6,564 | 7,473 |
| SH1 | native+ | 10.27 | 14.16 | 19.03 | 8.62 | 1 | 0.130 | 7,331 | 9,969 |
| SH1 | meilisearch | 7.02 | 9.63 | 11.66 | 7.05 | 1 | 0.120 | 5,391 | 7,259 |
| SH1 | solr | 4.80 | 6.57 | 8.41 | 4.95 | 1 | 0.097 | 8,149 | 10,269 |
| FD1 | native | 0.60 | 0.90 | 1.24 | 0.66 | 1 | 0.186 | 370 | 556 |
| FD1 | native+ | 0.64 | 0.97 | 1.13 | 0.67 | 1 | 0.040 | 387 | 426 |
| FD1 | meilisearch | 6.25 | 7.70 | 8.60 | 6.39 | 1 | 0.074 | 5,734 | 6,687 |
| FD1 | solr | 9.00 | 13.08 | 18.54 | 9.22 | 1 | 0.022 | 21,517 | 22,713 |
| FD3 | native | 0.74 | 1.13 | 1.38 | 0.80 | 1 | 0.021 | 516 | 543 |
| FD3 | native+ | 0.70 | 1.20 | 1.88 | 0.78 | 1 | 0.081 | 460 | 562 |
| FD3 | meilisearch | 5.61 | 7.50 | 8.32 | 5.95 | 1 | 0.044 | 5,114 | 5,690 |
| FD3 | solr | 6.39 | 9.38 | 10.34 | 6.50 | 1 | 0.087 | 12,859 | 15,939 |
| FD5 | native | 0.73 | 1.16 | 1.46 | 0.79 | 1 | 0.114 | 409 | 530 |
| FD5 | native+ | 0.72 | 1.08 | 1.48 | 0.78 | 1 | 0.008 | 504 | 513 |
| FD5 | meilisearch | 3.76 | 4.93 | 6.73 | 4.17 | 1 | 0.037 | 3,408 | 3,726 |
| FD5 | solr | 5.66 | 8.62 | 10.19 | 5.81 | 1 | 0.120 | 10,513 | 13,894 |

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

Ambient (host probe CPU ms, before → after each arm):

- run 1 meilisearch: 2026-09-29T07:53:15Z → 2026-09-29T08:06:36Z; probe 633 → 636 ms; load 1.42 1.18 1.16 1/465 425600 → 1.21 1.24 1.18 3/449 426555
- run 1 native: 2026-09-29T07:51:03Z → 2026-09-29T07:53:14Z; probe 739 → 772 ms; load 1.23 1.11 1.14 2/475 424918 → 1.42 1.18 1.16 2/465 425597
- run 1 solr: 2026-09-29T08:06:37Z → 2026-09-29T08:11:27Z; probe 756 → 794 ms; load 1.21 1.24 1.18 1/447 426566 → 1.45 1.56 1.36 3/397 427389
- run 2 meilisearch: 2026-09-29T08:18:45Z → 2026-09-29T08:32:23Z; probe 786 → 787 ms; load 1.30 1.55 1.49 1/405 428717 → 1.34 1.28 1.32 1/392 429738
- run 2 native: 2026-09-29T08:16:17Z → 2026-09-29T08:18:44Z; probe 686 → 807 ms; load 1.35 1.74 1.53 1/402 428390 → 1.30 1.55 1.49 3/406 428712
- run 2 solr: 2026-09-29T08:11:46Z → 2026-09-29T08:16:16Z; probe 846 → 822 ms; load 2.00 1.68 1.40 3/404 427607 → 1.29 1.74 1.53 2/401 428386
- run 3 meilisearch: 2026-09-29T08:32:38Z → 2026-09-29T08:46:45Z; probe 636 → 629 ms; load 1.33 1.28 1.32 1/394 429754 → 1.15 1.22 1.25 1/396 430498
- run 3 native: 2026-09-29T08:51:32Z → 2026-09-29T08:53:49Z; probe 743 → 744 ms; load 1.54 1.66 1.46 1/390 431074 → 1.22 1.46 1.41 1/391 431178
- run 3 solr: 2026-09-29T08:46:46Z → 2026-09-29T08:51:31Z; probe 744 → 727 ms; load 1.15 1.22 1.25 1/396 430501 → 1.54 1.66 1.46 2/390 431071
