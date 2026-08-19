# Experiment Loop

This repository is run as an evidence-producing research loop, not a feature backlog.

## Loop template

For every meaningful iteration, append an entry to `docs/experiments/LOG.md` with:

- **Question** — what uncertainty are we reducing?
- **Hypothesis** — falsifiable statement.
- **Workload** — exact fixture/dataset/query set and scale.
- **Metric(s)** — correctness, coverage, latency, memory, CPU, index size, relevance, etc.
- **Decision rule** — what result would advance, revise, or reject the approach?
- **Implementation** — smallest code path required to test it.
- **Results** — raw measurements, environment, commands, commit SHA.
- **Interpretation** — what the result supports and what it does not support.
- **Regression check** — which maintained tests/replays protect the finding?
- **Next question** — highest-value uncertainty now remaining.

## Priority order

Prefer experiments that attack the core thesis in this order:

1. **Semantic correctness** — can typed commerce semantics express the cases generic document matching gets wrong?
2. **Structural coverage** — how much realistic query intent can be resolved deterministically?
3. **Physical advantage** — do specialized structures reduce memory/CPU/latency at useful scale?
4. **Cold start** — can catalog profiling + semantic fuzzing create useful v1 context without per-SKU model calls?
5. **Learning loop** — can unresolved behavior produce safe candidate semantic routes that improve replay metrics?
6. **Scaling curve** — does the advantage survive 10x-ish workload increases before distributed architecture is introduced?

Do not spend substantial time on lower-priority polish while a higher-priority thesis question remains unanswered.

## Benchmark rules

- Benchmark release builds only unless explicitly testing debug behavior.
- Record CPU model/core count, RAM, OS, Rust version, dataset size, query count, warm/cold status, concurrency, and command line.
- Keep canonical benchmark fixtures versioned.
- Separate build/indexing measurements from serving measurements.
- Report percentiles, not averages alone.
- Include correctness/relevance guardrails beside performance results.
- Repeat noisy measurements enough to identify variance.
- When changing representation or planner behavior, rerun the maintained baseline workload.
- Never compare results generated from different query sets without labeling the comparison invalid or adjusted.

## Suggested scale ladder

Use the largest practical deterministic/public dataset available, but grow through a ladder rather than immediately chasing millions of products:

- tiny semantic fixtures: tens of products, exhaustive correctness;
- small: ~10k products, rapid iteration;
- medium: ~100k products, meaningful memory/latency profiling;
- target proof: ~500k products if cloud resources permit;
- stretch: 1M+ only after the earlier gates are useful.

Synthetic expansion is acceptable for performance scaling when clearly separated from relevance claims.

## Stop conditions

Stop adding implementation and write `SCALE_UP_DECISION.md` when any is true:

- the thesis is strong enough that the next question requires materially larger infrastructure/data or production engineering;
- a core assumption is falsified and further work would merely route around the evidence;
- repeated experiments converge on generic lexical/vector retrieval as the dominant architecture and commerce specialization adds insufficient benefit;
- the system is drifting toward a generic Lucene/Elasticsearch clone without a measured commerce-specific advantage.

## What not to do

- no autonomous architecture expansion just because something is technically interesting;
- no distributed cluster before single-node evidence demands it;
- no UI unless required to inspect an experiment and a CLI/report would not suffice;
- no benchmark-only special cases in production code paths;
- no undocumented dataset/query changes;
- no model-generated semantic route promoted without deterministic replay/evaluation evidence.
