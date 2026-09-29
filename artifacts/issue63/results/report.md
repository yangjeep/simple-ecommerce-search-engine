# Issue #63 report tables

Native per-request service floor = 396 µs CPU/query (FD1, Part A native FINAL median).

## Primitive table (500k unless noted; in-process thread CPU, median of 3 runs)

| primitive | CPU/op | allocs/op | bytes alloc/op | bytes touched (est.) | result | 100k→500k | same-window r (cell) | class |
|---|---|---|---|---|---|---|---|---|
| exact lookup (per lookup) | 229 ns | 0.00 | 0 | 256 | 1,000 | 1.16x | — | KEEP-provisional |
| single bitmap filter: color=white | 2,909 ns | 11.00 | 34,778 | 69,024 | 17,256 | 5.40x | 0.069 (FD1) | KEEP |
| single bitmap filter: color=white (borrowed) | 8 ns | 0.00 | 0 | 0 | 17,256 | 2.14x | — | KEEP-provisional |
| single bitmap filter: category Accent Chairs | 2,423 ns | 9.00 | 26,728 | 52,944 | 13,236 | 6.62x | — | KEEP-provisional |
| 3-way bitmap conjunction | 42.0 µs | 33.00 | 166,408 | 165,656 | 36 | 4.28x | 0.098 (FD3) | KEEP |
| 5-way conjunction (4 enums + rating range) | 4,329.8 µs | 150.00 | 526,931 | 6,335,528 | 12 | 4.38x | 0.144 (FD5) | KEEP |
| numeric range, broad (rating >= 4) | 3,875.6 µs | 106.00 | 328,000 | 6,203,200 | 383,604 | 4.12x | 1.151 (SH1) | REFINE |
| numeric range, narrow (review_count >= p99) | 276.0 µs | 69.00 | 22,848 | 72,576 | 4,032 | 4.67x | — | REFINE |
| same-variant conjunction (synthetic, 100k-derived) [100k] | 55.7 µs | 30.00 | 163,857 | 163,316 | 16,122 | — | — | REFINE |
| lexical residual: wood+bed | 110.5 µs | 26.00 | 156,448 | 131,072 | 12,432 | 7.86x | — | DELEGATE (reference; #57) |
| lexical residual: outdoor+dining+table | 49.9 µs | 34.00 | 201,816 | 196,238 | 1,116 | 4.45x | — | DELEGATE (reference; #57) |

- exact lookup (per lookup): per-op / 1000
- 5-way conjunction (4 enums + rating range): service cell FD5 = #77 filter_depth_5 = the 4 enum filters only
- numeric range, broad (rating >= 4): service cell SH1 = range + bounded sort
- same-variant conjunction (synthetic, 100k-derived): 100k tier only
- lexical residual: wood+bed: reference only
- lexical residual: outdoor+dining+table: reference only

## Residual attribution (Part A native FINAL, median-CPU run; server phase timers)

### FH3: service 37,219 µs CPU/query (server-timed total 35,636 µs)

| component | µs | % of service |
|---|---|---|
| match-all construction (P0 all_ordinals) | 13,388 | 36.0% |
| facet counting: iteration (B2 share 53%) | 11,644 | 31.3% |
| facet counting: gather (B2 share 2%) | 444 | 1.2% |
| facet counting: counting (B2 share 27%) | 5,999 | 16.1% |
| facet counting: materialization (B2 share 18%) | 3,986 | 10.7% |
| output assembly (bounded, 48 ids) | 172 | 0.5% |
| HTTP parse, JSON serialization, kernel, other | 1,583 | 4.3% |

### FH4: service 37,394 µs CPU/query (server-timed total 35,712 µs)

| component | µs | % of service |
|---|---|---|
| base candidates (color=black) | 33 | 0.1% |
| match-all construction (P0, color self-exclusion) | 12,130 | 32.4% |
| facet counting: iteration (B2 share 53%) | 12,341 | 33.0% |
| facet counting: gather (B2 share 2%) | 470 | 1.3% |
| facet counting: counting (B2 share 27%) | 6,358 | 17.0% |
| facet counting: materialization (B2 share 18%) | 4,225 | 11.3% |
| output assembly (bounded, 48 ids) | 154 | 0.4% |
| HTTP parse, JSON serialization, kernel, other | 1,682 | 4.5% |

### In-server vs hot-loop microbenchmark (same code)

| phase | in server (µs) | microbenchmark (µs) | ratio |
|---|---|---|---|
| FH3 match-all construction | 13,388.4 | 4,482.0 | 2.99x |
| FH3 color counting (facets phase vs B2 ordinal full|color) | 22,073.4 | 7,936.7 | 2.78x |
| FH3 N+ whole request (server total vs B1 pipeline p0r) | 21,025.9 | 9,169.6 | 2.29x |
| FD1 candidates (color=white clone) | 18.5 | 2.9 | 6.36x |

Server phase timers are wall-clock inside one request; for these CPU-bound single-threaded requests they agree with the cgroup CPU/query to within ~5% (HTTP/JSON/other row). The B2 split is a proportional model from the hot-loop microbenchmark, applied to the in-server facets phase.

