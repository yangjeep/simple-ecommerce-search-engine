# INVALID: first #66 heap pre-pass (exact F id-order check)

This run is **invalid** and is kept as evidence. It started 2026-09-30T07:38Z and was stopped at about 07:45Z, during the 512m launch. It ran before any envelope window.

**Result:** 1g was structurally equivalent (0 non-equivalences, 0 exclusions). But 478 of 480 F responses differed from the frozen `pools_frozen.json` ids in **exact order**.

**Why the check was wrong:**
- F ranks variant-level documents. Variants of one product share title and description, so their BM25 scores tie exactly.
- The order among tied documents follows internal docids, which can differ between fresh indexes.
- #65 never checked F id order across launches. Its in-load F check was `num_found` only, and its router identity check ran within a single launch.
- So the exact-order criterion in C7 as first implemented would have excluded every heap, including a fresh 3g index.

**Also:** the re-recorded pools were deleted by the first script, so the 1g ids are not available here.

**Replacement:** clarification C7a, a tie-aware check (`scripts/issue66/f_equiv.py`) against a fresh 3g control launch. Posted on #66 before rerunning.
