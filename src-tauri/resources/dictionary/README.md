# Enriched ECCEDICT data

The app embeds `enrichment.csv.gz` and `manifest.json` in Rust. It downloads the
pinned base CSV only when the user explicitly selects the dictionary download,
verifies its length and SHA-256, generates an enriched CSV locally, then imports
it into the existing dictionary redb database. Runtime Python/DuckDB is not used.

The compact CSV contains `word,phonetic,tag` overrides extracted from:

- Repository: <https://github.com/H1DDENADM1N/ECCEDICT>
- Revision: `7a83e99c45f0547be6391b79e8129f98f9c128d9`
- `output/phonetics.ddb`: UK/US phonetics (the upstream describes its source as
  Oxford 10th-edition dictionary data).
- `output/tag.ddb`: exam-list tags collected by the upstream from Qwerty Learner.

These are upstream dictionary data, not TransPortal's original annotations or
claims of official/current exam coverage. Base and additional tags are unioned;
meanings, examples, word forms and corpus information are preserved.

The upstream repository publishes an MIT license (included as `LICENSE.upstream`
and installed as a bundle resource). See the upstream repository
and its data-source descriptions when assessing downstream redistribution.

Rebuild from the repository root (development only):

```sh
uv run scripts/eccedict/build_enriched_csv.py --download \
  --cache-dir /tmp/opencode/eccedict \
  --out /tmp/opencode/eccedict/ecdict.enriched.csv \
  --overrides-out src-tauri/resources/dictionary/enrichment.csv.gz \
  --manifest-out src-tauri/resources/dictionary/manifest.json

uv run --no-project --with duckdb python -m unittest discover \
  -s scripts/eccedict -p '*_test.py'

# Optional full-data parity + redb import verification, without network I/O:
TRANSPORTAL_ECCEDICT_BASE=/tmp/opencode/eccedict/ecdict.csv \
TRANSPORTAL_ECCEDICT_ENRICHED=/tmp/opencode/eccedict/ecdict.enriched.csv \
  cargo test --manifest-path src-tauri/Cargo.toml \
  core::dictionary::enrichment::tests::default_manifest_and_optional_full_dataset_import -- --nocapture
```

Both the builder and runtime try public GHProxy mirrors before direct GitHub/CDN
routes. Mirror responses are verified against pinned hashes, never trusted merely
because an HTTP request succeeded. No API keys or private requests are proxied.
