Smoke checks at 100k (`catalog_3x`), run on 2026-09-29 before any preregistered #63 measurement to validate the harness. **Not evidence**: no number here feeds any rule, threshold or verdict. Timings are not retained.

- `equal_work_equivalence.json`: native FINAL and native `p2b` (a `cand_mode` smoke only; N⁺ was not chosen yet), plus Meilisearch (`I77_MEILI_LIKE_FOR_LIKE=1`) and Solr (`I63_EQUAL_WORK=1`). All 8 Part A cells are EQUIVALENT to the oracle for every engine.
- `negative_control_*`: Meilisearch with #77's **default** configuration. It returns 100 of 2,825 color values and full documents, so both cells are `NOT_EQUIVALENT_WORK`. This shows the checker detects exactly E3's unequal-work defect.
