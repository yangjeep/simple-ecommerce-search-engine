# Historical first-iteration summaries

These small files describe the completed pre-PR v1.1.1 experiment, not a new
benchmark of the PR checkout. See the report under `docs/experiments/`.
Large raw request/response/resource traces and public datasets are deliberately
not committed. Their absence means the summaries alone are not an independent
reproduction of every published number. Runnable generators/oracles are under
`scripts/athos80_validation`; fresh measurements must regenerate binary-bound
goldens and preserve their own complete evidence.

The bundled query excerpts and derived query-only quality cases come from
[Wayfair WANDS](https://github.com/wayfair/WANDS), commit
`3b74dcf4ba29ab8ff3e6a50b5b09fc627cb882b5`; see `WANDS_LICENSE.txt`.
No private code, merchant catalog or shopper behavior is included.
