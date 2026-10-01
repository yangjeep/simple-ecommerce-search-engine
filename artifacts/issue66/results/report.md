# Issue #66 — minimum resource envelope

**Preregistered gate (T2 = 100 QPS, S2): KEEP**


## Confirmed minima

| treatment | tier | S1 cores | S2 cores |
|---|---|---|---|
| b0 | 50 | INFEASIBLE | 1.5 |
| b0 | 100 | INFEASIBLE | 2 |
| b0 | 200 | INFEASIBLE | INFEASIBLE |
| h1 | 50 | 1 | 0.75 |
| h1 | 100 | 1 | 1 |
| h1 | 200 | 2 | 1.5 |

| treatment | S1 GiB (heap) | S2 GiB (heap) | joint S2 (cores, GiB, heap, passes) |
|---|---|---|---|
| b0 | INFEASIBLE | 1 (512m) | 2, 1, 512m, 0/3 |
| h1 | 5 (512m) | 5 (512m) | 1, 5, 512m, 2/3 |

## Ratios (H1 / B0)

- cores t50_S1: 0.333 (bound_b0_infeasible)
- cores t50_S2: 0.500 (exact)
- cores t100_S1: 0.333 (bound_b0_infeasible)
- cores t100_S2: 0.500 (exact)
- cores t200_S1: 0.667 (bound_b0_infeasible)
- cores t200_S2: 0.500 (bound_b0_infeasible)
- GiB T2/S2: 5.000 (lower_bound_b0_at_ladder_floor)
- normalized units u = max(cores, GiB/4) T2/S2: H1 1.25, B0 2.00, ratio 0.625 (flagged: not jointly confirmed)
- sensitivity shapes (non-gating): {"1:2": {"b0": 2.0, "h1": 2.5}, "1:8": {"b0": 2.0, "h1": 1.0}}
- limiting resource at T2/S2: {"b0": "cpu", "h1": "ram"}

## Descents

### CPU — primary mix, 50 QPS, b0

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | fail | PASS | latency | 8.0 | 51.3 | 97.1 | 0.0000 | 0.67 | 13.3 | 0.000 | 3.67 |
| 2.5 | PASS | PASS | none | 7.9 | 48.9 | 94.1 | 0.0000 | 0.63 | 12.6 | 0.000 | 3.67 |
| 2 | fail | PASS | latency | 8.2 | 51.4 | 101.1 | 0.0000 | 0.65 | 12.9 | 0.005 | 3.67 |
| 1.5 | fail | PASS | latency | 8.7 | 58.9 | 131.2 | 0.0000 | 0.69 | 13.7 | 0.041 | 3.68 |
| 1 | fail | fail | latency_throttled | 9.9 | 132.1 | 315.5 | 0.0000 | 0.69 | 13.9 | 0.353 | 3.69 |
| 0.75 | fail | fail | errors | 46.0 | 495.9 | 1244.8 | 0.0023 | 0.67 | 13.4 | 1.393 | 3.70 |

### CPU — primary mix, 50 QPS, h1

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | PASS | PASS | none | 3.7 | 22.8 | 53.2 | 0.0000 | 0.43 | 8.6 | 0.000 | 7.68 |
| 2.5 | PASS | PASS | none | 3.1 | 19.0 | 26.2 | 0.0000 | 0.34 | 6.9 | 0.000 | 7.67 |
| 2 | PASS | PASS | none | 2.5 | 17.7 | 21.7 | 0.0000 | 0.29 | 5.8 | 0.000 | 7.67 |
| 1.5 | PASS | PASS | none | 2.7 | 18.0 | 22.9 | 0.0000 | 0.29 | 5.8 | 0.000 | 7.67 |
| 1 | PASS | PASS | none | 2.7 | 17.7 | 22.1 | 0.0000 | 0.29 | 5.8 | 0.001 | 7.67 |
| 0.75 | PASS | PASS | none | 2.8 | 17.9 | 25.9 | 0.0000 | 0.29 | 5.8 | 0.018 | 7.67 |
| 0.5 | fail | PASS | latency_throttled | 3.8 | 37.7 | 118.6 | 0.0000 | 0.29 | 5.8 | 0.111 | 7.67 |

### CPU — primary mix, 100 QPS, b0

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | fail | PASS | latency | 8.6 | 65.7 | 129.4 | 0.0000 | 1.41 | 14.0 | 0.000 | 3.73 |
| 2.5 | fail | PASS | latency | 8.7 | 68.2 | 138.4 | 0.0000 | 1.41 | 13.9 | 0.019 | 3.74 |
| 2 | fail | PASS | latency_throttled | 8.8 | 83.3 | 190.2 | 0.0000 | 1.37 | 13.6 | 0.137 | 3.74 |
| 1.5 | fail | fail | errors | 33.5 | 330.6 | 947.3 | 0.0012 | 1.38 | 13.7 | 0.922 | 3.75 |
| 1 | fail | fail | errors | 3761.0 | 4040.2 | 4048.0 | 0.9928 | 1.00 | 1376.6 | 2.040 | 3.80 |

### CPU — primary mix, 100 QPS, h1

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | PASS | PASS | none | 4.2 | 21.5 | 38.5 | 0.0000 | 0.67 | 6.7 | 0.000 | 7.83 |
| 2.5 | PASS | PASS | none | 3.7 | 20.0 | 30.0 | 0.0000 | 0.64 | 6.3 | 0.000 | 7.83 |
| 2 | PASS | PASS | none | 3.6 | 19.2 | 26.8 | 0.0000 | 0.62 | 6.1 | 0.001 | 7.83 |
| 1.5 | PASS | PASS | none | 3.6 | 19.3 | 26.4 | 0.0000 | 0.62 | 6.1 | 0.002 | 7.83 |
| 1 | PASS | PASS | none | 3.7 | 20.4 | 61.8 | 0.0000 | 0.60 | 5.9 | 0.047 | 7.83 |
| 0.75 | fail | PASS | latency_throttled | 7.8 | 62.3 | 139.3 | 0.0000 | 0.60 | 5.9 | 0.328 | 7.84 |
| 0.5 | fail | fail | errors | 3793.4 | 4027.5 | 4045.2 | 0.9197 | 0.50 | 61.7 | 2.513 | 8.21 |

### CPU — primary mix, 200 QPS, b0

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | fail | fail | errors | 2198.7 | 3905.1 | 4026.3 | 0.7780 | 2.99 | 67.2 | 0.003 | 3.80 |
| 2.5 | fail | fail | errors | 2032.4 | 4045.1 | 4056.2 | 0.9998 | 2.50 | 49905.8 | 0.513 | 3.83 |

### CPU — primary mix, 200 QPS, h1

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | PASS | PASS | none | 5.3 | 26.4 | 54.3 | 0.0000 | 1.26 | 6.3 | 0.000 | 7.90 |
| 2.5 | PASS | PASS | none | 5.0 | 25.5 | 48.9 | 0.0000 | 1.27 | 6.3 | 0.002 | 7.90 |
| 2 | PASS | PASS | none | 4.2 | 22.0 | 32.6 | 0.0000 | 1.19 | 5.9 | 0.008 | 7.91 |
| 1.5 | PASS | PASS | none | 6.9 | 45.5 | 94.3 | 0.0000 | 1.19 | 5.9 | 0.179 | 7.91 |
| 1 | fail | fail | errors | 2102.8 | 3989.3 | 4035.8 | 0.8546 | 1.00 | 34.3 | 1.960 | 7.91 |
| 0.75 | fail | fail | errors | 2044.0 | 4037.9 | 4048.9 | 0.9532 | 0.75 | 79.9 | 2.183 | 7.92 |

### CPU — structural mix, 100 QPS, b0

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | fail | PASS | latency | 7.9 | 58.6 | 103.8 | 0.0000 | 1.25 | 12.7 | 0.000 | 3.81 |
| 2.5 | fail | PASS | latency | 8.2 | 58.2 | 108.4 | 0.0000 | 1.26 | 12.8 | 0.009 | 3.81 |
| 2 | fail | PASS | latency_throttled | 8.0 | 76.0 | 147.4 | 0.0000 | 1.24 | 12.6 | 0.074 | 3.82 |
| 1.5 | fail | fail | latency_throttled | 10.7 | 172.2 | 467.2 | 0.0000 | 1.24 | 12.6 | 0.473 | 3.83 |
| 1 | fail | fail | errors | 3543.9 | 4033.9 | 4046.5 | 0.9874 | 1.00 | 803.7 | 2.035 | 3.88 |

### CPU — structural mix, 100 QPS, h1

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | PASS | PASS | none | 2.0 | 22.4 | 40.4 | 0.0000 | 0.57 | 5.8 | 0.000 | 7.88 |
| 2.5 | PASS | PASS | none | 1.7 | 19.6 | 32.4 | 0.0000 | 0.56 | 5.7 | 0.000 | 7.88 |
| 2 | PASS | PASS | none | 1.6 | 18.5 | 23.9 | 0.0000 | 0.49 | 5.0 | 0.000 | 7.88 |
| 1.5 | PASS | PASS | none | 1.6 | 18.4 | 24.0 | 0.0000 | 0.47 | 4.8 | 0.001 | 7.88 |
| 1 | PASS | PASS | none | 1.6 | 18.9 | 26.5 | 0.0000 | 0.48 | 4.9 | 0.014 | 7.88 |
| 0.75 | PASS | PASS | none | 1.8 | 24.8 | 82.5 | 0.0000 | 0.47 | 4.8 | 0.120 | 7.88 |
| 0.5 | fail | fail | errors | 3553.5 | 4027.8 | 4044.8 | 0.8139 | 0.50 | 27.3 | 2.387 | 7.89 |

### CPU — lexical mix, 100 QPS, b0

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | fail | PASS | latency | 9.4 | 52.5 | 110.8 | 0.0000 | 1.32 | 13.3 | 0.000 | 3.89 |
| 2.5 | fail | PASS | latency | 9.3 | 52.4 | 117.1 | 0.0000 | 1.31 | 13.2 | 0.010 | 3.90 |
| 2 | fail | PASS | latency_throttled | 9.6 | 65.0 | 144.7 | 0.0000 | 1.32 | 13.3 | 0.079 | 3.90 |
| 1.5 | fail | fail | latency_throttled | 13.9 | 147.7 | 332.6 | 0.0000 | 1.30 | 13.0 | 0.574 | 3.91 |
| 1 | fail | fail | errors | 3794.1 | 4040.2 | 4053.2 | 0.9938 | 1.00 | 1619.2 | 2.033 | 4.04 |

### CPU — lexical mix, 100 QPS, h1

| cores | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 3 | PASS | PASS | none | 8.9 | 21.4 | 37.1 | 0.0000 | 0.78 | 7.8 | 0.000 | 7.99 |
| 2.5 | PASS | PASS | none | 9.2 | 19.8 | 30.3 | 0.0000 | 0.78 | 7.8 | 0.000 | 7.99 |
| 2 | PASS | PASS | none | 8.8 | 17.9 | 24.4 | 0.0000 | 0.72 | 7.2 | 0.001 | 8.00 |
| 1.5 | PASS | PASS | none | 8.8 | 18.1 | 24.9 | 0.0000 | 0.72 | 7.2 | 0.003 | 8.00 |
| 1 | PASS | PASS | none | 9.0 | 31.4 | 76.0 | 0.0000 | 0.72 | 7.2 | 0.109 | 8.01 |
| 0.75 | fail | fail | errors | 49.5 | 582.0 | 1223.6 | 0.0048 | 0.72 | 7.2 | 1.332 | 8.01 |
| 0.5 | fail | fail | errors | 3967.5 | 4043.1 | 4048.3 | 0.9832 | 0.50 | 297.7 | 2.536 | 8.26 |

### RAM — heap 3g, 100 QPS, 3 cores, b0

| GiB | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | fail | PASS | latency | 8.5 | 62.8 | 121.0 | 0.0000 | 1.37 | 13.6 | 0.000 | 3.93 |
| 8 | fail | PASS | latency | 8.7 | 64.2 | 123.4 | 0.0000 | 1.38 | 13.7 | 0.000 | 3.94 |
| 7 | fail | PASS | latency | 8.2 | 58.2 | 112.6 | 0.0000 | 1.31 | 12.9 | 0.000 | 3.95 |
| 6 | fail | PASS | latency | 8.3 | 58.0 | 112.9 | 0.0000 | 1.32 | 13.1 | 0.000 | 3.95 |
| 5 | fail | PASS | latency | 8.2 | 55.7 | 112.1 | 0.0000 | 1.30 | 12.9 | 0.000 | 3.96 |
| 4 | fail | PASS | latency | 8.1 | 57.4 | 111.5 | 0.0000 | 1.30 | 12.8 | 0.000 | 3.97 |
| 3 | fail | fail | oom | 0.3 | 0.5 | 0.8 | 1.0000 | 0.00 | 0.0 | 0.000 | 0.00 |

### RAM — heap 3g, 100 QPS, 3 cores, h1

| GiB | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | PASS | PASS | none | 4.2 | 21.3 | 37.9 | 0.0000 | 0.69 | 6.8 | 0.000 | 7.77 |
| 8 | PASS | PASS | none | 3.5 | 19.7 | 27.0 | 0.0000 | 0.63 | 6.3 | 0.000 | 7.78 |
| 7 | fail | fail | oom | 0.3 | 0.5 | 0.8 | 1.0000 | 0.01 | 1754.4 | 0.000 | 3.31 |

### RAM — heap 1g, 100 QPS, 3 cores, b0

| GiB | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | fail | PASS | latency | 8.5 | 62.4 | 121.6 | 0.0000 | 1.37 | 13.6 | 0.000 | 1.71 |
| 8 | fail | PASS | latency | 8.5 | 60.4 | 118.2 | 0.0000 | 1.33 | 13.2 | 0.000 | 1.72 |
| 7 | fail | PASS | latency | 8.9 | 71.2 | 131.8 | 0.0000 | 1.34 | 13.2 | 0.000 | 1.73 |
| 6 | fail | PASS | latency | 8.4 | 58.5 | 114.5 | 0.0000 | 1.30 | 12.9 | 0.000 | 1.74 |
| 5 | fail | PASS | latency | 8.8 | 60.6 | 118.8 | 0.0000 | 1.37 | 13.6 | 0.000 | 1.74 |
| 4 | fail | PASS | latency | 8.7 | 59.6 | 112.0 | 0.0000 | 1.34 | 13.3 | 0.000 | 1.75 |
| 3 | fail | PASS | latency | 8.2 | 56.1 | 108.1 | 0.0000 | 1.28 | 12.7 | 0.000 | 1.76 |
| 2 | fail | PASS | latency | 8.4 | 56.0 | 110.4 | 0.0000 | 1.30 | 12.9 | 0.000 | 1.77 |
| 1.5 | fail | PASS | latency | 8.8 | 65.8 | 120.1 | 0.0000 | 1.36 | 13.5 | 0.000 | 1.50 |
| 1 | fail | fail | oom | 0.3 | 0.5 | 0.8 | 1.0000 | 0.00 | 0.0 | 0.000 | 0.00 |

### RAM — heap 1g, 100 QPS, 3 cores, h1

| GiB | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | PASS | PASS | none | 3.6 | 19.0 | 25.4 | 0.0000 | 0.63 | 6.2 | 0.000 | 5.75 |
| 8 | PASS | PASS | none | 3.6 | 18.6 | 24.6 | 0.0000 | 0.61 | 6.0 | 0.000 | 5.76 |
| 7 | PASS | PASS | none | 3.6 | 18.5 | 24.5 | 0.0000 | 0.60 | 6.0 | 0.000 | 5.76 |
| 6 | PASS | PASS | none | 3.5 | 18.4 | 24.6 | 0.0000 | 0.60 | 5.9 | 0.000 | 5.76 |
| 5 | fail | fail | oom | 0.3 | 0.5 | 0.9 | 1.0000 | 0.01 | 1605.2 | 0.000 | 1.26 |

### RAM — heap 512m, 100 QPS, 3 cores, b0

| GiB | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | fail | PASS | latency | 8.9 | 63.2 | 121.9 | 0.0000 | 1.37 | 13.6 | 0.000 | 1.16 |
| 8 | fail | PASS | latency | 9.0 | 62.8 | 117.9 | 0.0000 | 1.38 | 13.6 | 0.000 | 1.17 |
| 7 | fail | PASS | latency | 9.1 | 60.3 | 114.9 | 0.0000 | 1.37 | 13.6 | 0.000 | 1.18 |
| 6 | fail | PASS | latency | 8.7 | 59.1 | 115.6 | 0.0000 | 1.34 | 13.2 | 0.000 | 1.19 |
| 5 | fail | PASS | latency | 8.8 | 63.5 | 120.7 | 0.0000 | 1.37 | 13.5 | 0.000 | 1.20 |
| 4 | fail | PASS | latency | 8.8 | 62.4 | 120.6 | 0.0000 | 1.36 | 13.5 | 0.000 | 1.20 |
| 3 | fail | PASS | latency | 8.8 | 61.0 | 114.8 | 0.0000 | 1.35 | 13.4 | 0.000 | 1.21 |
| 2 | fail | PASS | latency | 8.7 | 59.0 | 115.1 | 0.0000 | 1.34 | 13.3 | 0.000 | 1.22 |
| 1.5 | fail | PASS | latency | 8.8 | 67.8 | 125.4 | 0.0000 | 1.36 | 13.5 | 0.000 | 1.23 |
| 1 | fail | PASS | latency | 8.9 | 63.2 | 120.6 | 0.0000 | 1.35 | 13.4 | 0.000 | 1.00 |

### RAM — heap 512m, 100 QPS, 3 cores, h1

| GiB | S1 | S2 | mode | P50 | P95 | P99 | err | cores used | ms/q | throttled | mem GiB |
|---|---|---|---|---|---|---|---|---|---|---|---|
| 12 | PASS | PASS | none | 3.7 | 19.8 | 27.4 | 0.0000 | 0.64 | 6.4 | 0.000 | 5.24 |
| 8 | PASS | PASS | none | 3.8 | 20.4 | 29.9 | 0.0000 | 0.64 | 6.3 | 0.000 | 5.24 |
| 7 | PASS | PASS | none | 3.7 | 19.7 | 27.0 | 0.0000 | 0.63 | 6.3 | 0.000 | 5.24 |
| 6 | PASS | PASS | none | 3.6 | 19.5 | 26.6 | 0.0000 | 0.62 | 6.2 | 0.000 | 5.25 |
| 5 | PASS | PASS | none | 3.7 | 19.7 | 26.1 | 0.0000 | 0.63 | 6.2 | 0.000 | 4.99 |
| 4 | fail | fail | oom | 0.3 | 0.6 | 1.0 | 1.0000 | 0.01 | 1692.0 | 0.000 | 0.75 |


## Confirmation runs

### cpu_confirm

| job | run | S1 | S2 | mode | P95 | P99 | cores used | mem GiB |
|---|---|---|---|---|---|---|---|---|
| b0_cpu_primary_t50_h3g_1.5 | 1 | fail | PASS | latency | 58.4 | 129.2 | 0.70 | 3.66 |
| b0_cpu_primary_t50_h3g_1.5 | 2 | fail | PASS | latency_throttled | 61.7 | 126.4 | 0.71 | 3.67 |
| b0_cpu_primary_t50_h3g_1.5 | 3 | fail | PASS | latency | 58.8 | 121.6 | 0.69 | 3.70 |
| h1_cpu_primary_t50_h3g_0.75 | 1 | fail | PASS | latency_throttled | 52.5 | 113.8 | 0.42 | 7.81 |
| h1_cpu_primary_t50_h3g_0.75 | 2 | fail | PASS | latency_throttled | 47.7 | 115.4 | 0.39 | 7.60 |
| h1_cpu_primary_t50_h3g_0.75 | 3 | fail | PASS | latency_throttled | 62.0 | 152.9 | 0.42 | 7.82 |
| h1_cpu_primary_t50_h3g_0.5 | 1 | fail | fail | latency_throttled | 355.6 | 589.8 | 0.40 | 7.74 |
| h1_cpu_primary_t50_h3g_0.5 | 2 | fail | fail | errors | 2059.9 | 2064.0 | 0.50 | 7.57 |
| h1_cpu_primary_t50_h3g_0.5 | 3 | fail | fail | latency_throttled | 292.6 | 542.6 | 0.39 | 7.80 |
| b0_cpu_primary_t100_h3g_2 | 1 | fail | PASS | latency_throttled | 87.2 | 197.0 | 1.39 | 3.72 |
| b0_cpu_primary_t100_h3g_2 | 2 | fail | fail | latency_throttled | 94.4 | 214.0 | 1.45 | 3.72 |
| b0_cpu_primary_t100_h3g_2 | 3 | fail | PASS | latency_throttled | 79.1 | 172.5 | 1.38 | 3.76 |
| h1_cpu_primary_t100_h3g_1 | 1 | fail | fail | latency_throttled | 114.9 | 313.2 | 0.71 | 7.78 |
| h1_cpu_primary_t100_h3g_1 | 2 | PASS | PASS | none | 25.6 | 71.5 | 0.65 | 7.79 |
| h1_cpu_primary_t100_h3g_1 | 3 | PASS | PASS | none | 27.2 | 95.8 | 0.63 | 7.81 |
| h1_cpu_primary_t100_h3g_0.75 | 1 | fail | fail | latency_throttled | 360.2 | 622.3 | 0.66 | 7.80 |
| h1_cpu_primary_t100_h3g_0.75 | 2 | fail | fail | errors | 2687.8 | 2823.4 | 0.75 | 7.88 |
| h1_cpu_primary_t100_h3g_0.75 | 3 | fail | fail | latency_throttled | 522.0 | 955.7 | 0.66 | 7.81 |
| h1_cpu_primary_t200_h3g_1.5 | 1 | fail | fail | latency_throttled | 322.2 | 570.4 | 1.29 | 7.78 |
| h1_cpu_primary_t200_h3g_1.5 | 2 | fail | PASS | latency_throttled | 54.8 | 157.3 | 1.19 | 7.81 |
| h1_cpu_primary_t200_h3g_1.5 | 3 | fail | PASS | latency_throttled | 56.0 | 132.9 | 1.23 | 7.89 |

### cpu_confirm_stepup

| job | run | S1 | S2 | mode | P95 | P99 | cores used | mem GiB |
|---|---|---|---|---|---|---|---|---|
| h1_cpu_primary_t50_h3g_1 | 1 | PASS | PASS | none | 20.0 | 46.9 | 0.39 | 7.83 |
| h1_cpu_primary_t50_h3g_1 | 2 | PASS | PASS | none | 19.1 | 47.4 | 0.38 | 7.82 |
| h1_cpu_primary_t50_h3g_1 | 3 | PASS | PASS | none | 19.3 | 39.1 | 0.39 | 7.81 |
| h1_cpu_primary_t50_h3g_0.75 | 1 | fail | PASS | latency_throttled | 59.0 | 126.4 | 0.41 | 7.82 |
| h1_cpu_primary_t50_h3g_0.75 | 2 | fail | PASS | latency_throttled | 57.4 | 156.4 | 0.40 | 7.85 |
| h1_cpu_primary_t50_h3g_0.75 | 3 | fail | PASS | latency_throttled | 48.9 | 111.5 | 0.39 | 7.81 |
| h1_cpu_primary_t100_h3g_1 | 1 | PASS | PASS | none | 25.5 | 81.0 | 0.64 | 7.83 |
| h1_cpu_primary_t100_h3g_1 | 2 | PASS | PASS | none | 26.6 | 95.4 | 0.63 | 7.83 |
| h1_cpu_primary_t100_h3g_1 | 3 | fail | fail | latency_throttled | 80.6 | 315.2 | 0.68 | 7.83 |
| h1_cpu_primary_t200_h3g_2 | 1 | PASS | PASS | none | 23.3 | 38.1 | 1.23 | 7.86 |
| h1_cpu_primary_t200_h3g_2 | 2 | PASS | PASS | none | 23.6 | 43.7 | 1.22 | 7.91 |
| h1_cpu_primary_t200_h3g_2 | 3 | PASS | PASS | none | 24.6 | 44.9 | 1.23 | 7.91 |

### mem_confirm

| job | run | S1 | S2 | mode | P95 | P99 | cores used | mem GiB |
|---|---|---|---|---|---|---|---|---|
| b0_mem_primary_t100_h512m_1 | 1 | fail | PASS | latency | 62.3 | 119.2 | 1.37 | 1.00 |
| b0_mem_primary_t100_h512m_1 | 2 | fail | PASS | latency | 67.1 | 122.8 | 1.40 | 1.00 |
| b0_mem_primary_t100_h512m_1 | 3 | fail | PASS | latency | 65.5 | 124.8 | 1.40 | 1.00 |
| h1_mem_primary_t100_h512m_5 | 1 | PASS | PASS | none | 23.5 | 34.9 | 0.67 | 5.00 |
| h1_mem_primary_t100_h512m_5 | 2 | PASS | PASS | none | 19.6 | 26.6 | 0.64 | 4.99 |
| h1_mem_primary_t100_h512m_5 | 3 | PASS | PASS | none | 21.6 | 32.1 | 0.67 | 4.99 |

### joint

| job | run | S1 | S2 | mode | P95 | P99 | cores used | mem GiB |
|---|---|---|---|---|---|---|---|---|
| b0_joint_primary_t100_h512m_1 | 1 | fail | fail | latency_throttled | 100.8 | 257.7 | 1.45 | 1.00 |
| b0_joint_primary_t100_h512m_1 | 2 | fail | fail | latency_throttled | 123.7 | 295.9 | 1.52 | 1.00 |
| b0_joint_primary_t100_h512m_1 | 3 | fail | fail | latency_throttled | 91.7 | 215.9 | 1.43 | 1.00 |
| h1_joint_primary_t100_h512m_5 | 1 | fail | fail | latency_throttled | 91.1 | 220.6 | 0.73 | 4.99 |
| h1_joint_primary_t100_h512m_5 | 2 | fail | PASS | latency_throttled | 47.4 | 142.8 | 0.67 | 4.99 |
| h1_joint_primary_t100_h512m_5 | 3 | fail | PASS | latency_throttled | 57.9 | 167.1 | 0.71 | 4.99 |

