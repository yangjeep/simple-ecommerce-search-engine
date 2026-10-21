# Independent prototype validation

Python 3.11+ standard library; Rust 1.90+; Linux `/proc` is required by the owned
HTTP/resource harness. Commands below run only local test processes. Never point
this harness at production or kill an unrelated listener. Use a fresh output
path each time. No raw public catalog or private data is committed here.

## Offline checks (no service, no dataset download)

From repository root:

```sh
python3 -m unittest discover -s scripts/athos80_validation -p 'test_*.py'
python3 scripts/athos80_verify_query_provenance.py
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo build --locked --workspace --release
```

The Python tests include hand-calculated oracle outcomes, mocked scheduler and
ownership failures, safe loopback handling, CSV quoting and all 480 bundled public
quality query cases. They do not establish engine performance or quality.

## Local synthetic HTTP correctness

```sh
cargo build --locked -p athos80-eval --features bm25 --bins
python3 scripts/athos80_http_smoke.py "$PWD/target/debug/athos80_server" "$PWD/target/athos80-smoke"
python3 scripts/athos80_validation/prepare_fixtures.py
BINARY="$PWD/target/debug/athos80_server"
BINARY_SHA=$(sha256sum "$BINARY" | cut -d' ' -f1)
python3 scripts/athos80_validation/run_correctness.py --run-approved \
  --binary "$BINARY" --expect-binary-sha256 "$BINARY_SHA" \
  --phase synthetic --port 18890 --output target/athos80-synthetic
python3 scripts/athos80_validation/run_correctness.py --run-approved \
  --binary "$BINARY" --expect-binary-sha256 "$BINARY_SHA" \
  --phase global --port 18890 --output target/athos80-global
```

The wrappers check that the port is free, launch their own child, validate full
responses, verify frozen input hashes, and stop/reap only that child. Generated
synthetic fixtures are ignored by Git. The Rust contract and schema/examples are
in `crates/athos80-eval`.

## Optional WANDS preparation and correctness

The existing downloader pins public WANDS, verifies checksums, and writes roughly
100 MB to ignored `dataset_cache/wands`. WANDS is MIT licensed; see
`artifacts/athos80/WANDS_LICENSE.txt`. The prototype's derived brand/inventory,
variant/price/rule/factor/traffic fields are synthetic, not merchant data.

```sh
bash scripts/datasets/fetch_wands.sh
python3 scripts/datasets/prepare_wands.py
python3 scripts/athos80_validation/prepare_wands.py \
  --source dataset_cache/wands/catalog.jsonl \
  --queries dataset_cache/wands/query.csv --out target/athos80-wands
cargo build --locked -p athos80-eval --release --features bm25 --bins
BINARY="$PWD/target/release/athos80_server"
BINARY_SHA=$(sha256sum "$BINARY" | cut -d' ' -f1)
python3 scripts/athos80_validation/run_correctness.py --run-approved \
  --binary "$BINARY" --expect-binary-sha256 "$BINARY_SHA" \
  --phase wands --data-root target/athos80-wands --port 18890 \
  --output target/athos80-wands-correctness
python3 scripts/athos80_validation/build_goldens.py \
  --run-directory target/athos80-wands-correctness \
  --workload target/athos80-wands/workload.json --output target/athos80-goldens.json
```

Expect 1,924 checks for 481 cases × four arms. Complete BM25 traces supply scores
to the independent structural/ranking oracle: this is not an independent BM25
implementation or relevance judgment. Generated metadata includes local paths,
so regenerated workload hashes need not equal historical archive hashes. Never
mix new binaries/workloads with old goldens.

## Resources and quality

`resource_runner.py --help` describes explicit binary, catalog, workload, golden,
evidence-file, offered-load, SLO and generator thresholds. It is a Linux loopback
runner, not a VM provisioner. Start only after correctness, freeze a protocol and
record all service CPU/memory. Missing SLO thresholds yield accounting-only output,
never a capacity pass. Current operator-facing wording differs from the archived
runner; do not claim identical source hashes. `quality_collector.py` collects the
separate pure 480-query corpus under explicit input hashes; it does not score
relevance. Full quality re-scoring additionally requires the official qrels and
recorded responses; they are not bundled here.

Historical measurements and limitations: `docs/experiments/ATHOS80_FIRST_ITERATION.md`.
