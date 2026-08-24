# Documentation map

The repository has a long research history. The docs are intentionally split by **purpose**, not by every project phase.

If you only want to understand the project, read three files:

1. [`../README.md`](../README.md) — one-minute overview.
2. [`WHY.md`](WHY.md) — why the architecture narrowed to its current form.
3. [`architecture/README.md`](architecture/README.md) — what actually exists in `main` today.

## Overview

| Document | Use it for |
|---|---|
| [`WHY.md`](WHY.md) | The thesis, what failed, and why the current direction exists. |
| [`WHAT.md`](WHAT.md) | The system boundary: production/core code vs. experimental or deliberately delegated work. |
| [`EXPERIMENT_LOOP.md`](EXPERIMENT_LOOP.md) | The research discipline used to preregister, measure, adversarially review, and preserve negative results. |

## Architecture

[`architecture/`](architecture/) describes the current implementation and component boundaries.

[`adr/`](adr/) contains architectural decision records. ADRs are historical decisions: later evidence can supersede them, but they are not rewritten to pretend the project always knew the final answer.

## Decisions

[`decisions/`](decisions/) contains the compact decision records for each major research phase/issue. These are the right place to answer **“what did that phase conclude?”** without reading raw experiment logs.

Start with [`decisions/README.md`](decisions/README.md).

## Experiments

[`experiments/`](experiments/) contains preregistered protocols and append-only experiment logs. They are intentionally detailed and may include superseded measurements, methodology defects, reruns, and adversarial-review corrections.

Use them when you need to audit a claim rather than understand the project quickly.

## Research

[`research/`](research/) contains exploratory material that informs experiments but is not itself a terminal system decision: prior-art archaeology, paper notes, economic models, methodology work, and domain-neutral representation research.

## Evidence and reproducibility

The documentation points into several non-doc directories on purpose:

- [`../benchmarks/manifests/`](../benchmarks/manifests/) — experiment definitions / reproduction metadata.
- [`../artifacts/manifests/`](../artifacts/manifests/) — result manifests.
- [`research/artifacts/`](research/artifacts/) — archived research outputs too large/noisy for the narrative docs.
- [`../scripts/`](../scripts/) — dataset acquisition and experiment helpers.

## Simple rule for adding new docs

- **Current system explanation** → `architecture/`
- **A decision / verdict** → `decisions/`
- **Protocol or experiment journal** → `experiments/`
- **Exploratory analysis / prior art / paper work** → `research/`
- **Durable architecture choice** → `adr/`
- **Project-level summary** → keep it short and put it at `docs/` root

Do not create a new documentation category for one issue or one phase.
