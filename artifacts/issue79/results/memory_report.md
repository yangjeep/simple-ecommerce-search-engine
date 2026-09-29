## Memory / build accounting (median of 3 launches; memory.current after load, 2 s settle)

| configuration | RSS after load MiB | peak after load MiB | launch->ready s | index build ms | sort columns bytes | presence bytes | columns build ms | presence build ms |
|---|---|---|---|---|---|---|---|---|
| N0 binary (#77, from N0 runs) | 4206.1 | 4206.1 | 107.6 | - | - | - | - | - |
| base | 4206.0 (delta vs base +0.0) | 4206.0 | 96.4 | 85818 | 0 | 0 | 0.0 | 0.0 |
| facet_only | 4206.0 (delta vs base +0.0) | 4206.0 | 91.2 | 81738 | 0 | 0 | 0.0 | 0.0 |
| sort_columns_only | 4206.0 (delta vs base -0.1) | 4206.0 | 92.0 | 82563 | 12382272 | 0 | 10.4 | 0.0 |
| presence_only | 4206.0 (delta vs base -0.1) | 4206.0 | 100.8 | 91242 | 0 | 196824 | 0.0 | 52.1 |
| combined | 4206.5 (delta vs base +0.5) | 4206.5 | 93.8 | 84294 | 12382272 | 196824 | 10.6 | 61.6 |

index_approx_bytes (on-heap estimate) = 116915570, ordinal_facet_approx_bytes = 29361380, N = 515928
