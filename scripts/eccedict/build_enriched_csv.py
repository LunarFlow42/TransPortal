# /// script
# requires-python = ">=3.10"
# dependencies = ["duckdb"]
# ///
"""Prepare the default enriched dictionary and its compact runtime overrides.

DuckDB is a development-time dependency only. The desktop app verifies and
downloads the pinned base CSV, applies enrichment.csv.gz with Rust, and imports
the resulting enriched CSV without Python, DuckDB, or a separate data service.

Local fork checkout:
    uv run build_enriched_csv.py --out ecdict.enriched.csv

Rebuild the checked-in enrichment asset from the pinned upstream inputs:
    uv run build_enriched_csv.py --download --cache-dir /tmp/opencode/eccedict \
        --out /tmp/opencode/eccedict/ecdict.enriched.csv \
        --overrides-out src-tauri/resources/dictionary/enrichment.csv.gz \
        --manifest-out src-tauri/resources/dictionary/manifest.json
"""

from __future__ import annotations

import argparse
import csv
import gzip
import hashlib
import io
import json
import os
import sys
import time
import urllib.request
from pathlib import Path

import duckdb

# Column indices in ecdict.csv / the enriched output.
COL_WORD = 0
COL_PHONETIC = 1
COL_TAG = 7
UPSTREAM = "H1DDENADM1N/ECCEDICT"
REVISION = "7a83e99c45f0547be6391b79e8129f98f9c128d9"
# Git blob IDs and lengths from the pinned upstream tree, not mutable master.
INPUTS = {
    "ecdict.csv": ("c4ade63ea08cf39d9c3475e96929036d64d94c94", 65_933_428),
    "output/phonetics.ddb": ("bf453b3558337435ac23d79812de0136431ec319", 27_275_264),
    "output/tag.ddb": ("edd1b271cf227e86a224fa38a3982f0afc124596", 2_109_440),
}
HEADER = [
    "word", "phonetic", "definition", "translation", "pos", "collins", "oxford",
    "tag", "bnc", "frq", "exchange", "detail", "audio",
]


def load_phonetics(path: Path) -> dict[str, tuple[str | None, str | None]]:
    """word -> (uk, us) IPA from `output/phonetics.ddb` (DuckDB)."""
    con = duckdb.connect(str(path), read_only=True)
    try:
        rows = con.execute("SELECT word, phon_uk, phon_us FROM words").fetchall()
    finally:
        con.close()
    return {word.strip().lower(): (uk, us) for word, uk, us in rows}


def load_tags(path: Path) -> dict[str, str]:
    """word -> space separated exam tags from `output/tag.ddb` (DuckDB)."""
    con = duckdb.connect(str(path), read_only=True)
    try:
        rows = con.execute("SELECT word, tag FROM words").fetchall()
    finally:
        con.close()
    return {word.strip().lower(): tag for word, tag in rows if tag}


def digest(path: Path, algorithm: str = "sha256") -> str:
    hasher = hashlib.new(algorithm)
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            hasher.update(chunk)
    return hasher.hexdigest()


def matches_input(path: Path, blob: str, size: int) -> bool:
    if not path.is_file() or path.stat().st_size != size:
        return False
    hasher = hashlib.sha1(f"blob {size}\0".encode("ascii"))
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1 << 20), b""):
            hasher.update(chunk)
    return hasher.hexdigest() == blob


def download_inputs(cache: Path) -> None:
    for name, (blob, size) in INPUTS.items():
        path = cache / name
        if matches_input(path, blob, size):
            continue
        path.parent.mkdir(parents=True, exist_ok=True)
        temporary = path.with_suffix(path.suffix + ".part")
        last_error = ""
        for url in download_urls(name):
            try:
                print(f"downloading {name} via {url.split('/')[2]}", file=sys.stderr)
                request = urllib.request.Request(url, headers={"User-Agent": "TransPortal-data-builder"})
                started = time.monotonic()
                with urllib.request.urlopen(request, timeout=30) as response, temporary.open("wb") as target:
                    for chunk in iter(lambda: response.read(1 << 20), b""):
                        if time.monotonic() - started > 180:
                            raise TimeoutError("dictionary mirror exceeded the download budget")
                        target.write(chunk)
                if not matches_input(temporary, blob, size):
                    raise ValueError(f"pinned input verification failed: {name}")
                temporary.replace(path)
                break
            except (OSError, ValueError) as error:
                last_error = str(error)
                print(f"mirror failed: {last_error}", file=sys.stderr)
                temporary.unlink(missing_ok=True)
        else:
            raise RuntimeError(f"cannot fetch {name}: {last_error}")


def download_urls(name: str) -> tuple[str, ...]:
    raw = f"https://raw.githubusercontent.com/{UPSTREAM}/{REVISION}/{name}"
    return (
        f"https://gh-proxy.com/{raw}",
        f"https://ghproxy.net/{raw}",
        raw,
        f"https://cdn.jsdelivr.net/gh/{UPSTREAM}@{REVISION}/{name}",
    )


def format_phonetics(uk: str | None, us: str | None) -> str:
    parts = []
    for label, value in (("英", uk), ("美", us)):
        if value and value.strip().strip("/"):
            parts.append(f"{label} /{value.strip().strip('/')}/")
    return " ".join(parts)


def merge_tags(original: str, additional: str) -> str:
    # Preserve tags already present in ECDICT; do not replace them with the
    # subset of exam lists supplied by the fork.
    return " ".join(sorted(set(original.lower().split()) | set(additional.lower().split())))


def write_overrides(
    path: Path,
    phonetics: dict[str, tuple[str | None, str | None]],
    tags: dict[str, str],
) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    count = 0
    # Empty filename and fixed mtime keep the generated asset reproducible.
    with path.open("wb") as raw, gzip.GzipFile(filename="", mode="wb", fileobj=raw, mtime=0) as compressed:
        with io.TextIOWrapper(compressed, encoding="utf-8", newline="") as output:
            writer = csv.writer(output, lineterminator="\n")
            writer.writerow(["word", "phonetic", "tag"])
            for word in sorted(phonetics.keys() | tags.keys()):
                phonetic = format_phonetics(*phonetics.get(word, (None, None)))
                tag = merge_tags("", tags.get(word, ""))
                if phonetic or tag:
                    writer.writerow([word, phonetic, tag])
                    count += 1
    return count


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dict", default="ecdict.csv", help="source ecdict.csv")
    parser.add_argument("--phonetics", default="output/phonetics.ddb")
    parser.add_argument("--tags", default="output/tag.ddb")
    parser.add_argument("--out", default="ecdict.enriched.csv")
    parser.add_argument("--download", action="store_true", help="download verified pinned upstream inputs")
    parser.add_argument("--cache-dir", type=Path, default=Path(".cache/eccedict"))
    parser.add_argument("--overrides-out", type=Path, help="write compact gzipped CSV overrides for the app")
    parser.add_argument("--manifest-out", type=Path, help="write the runtime data manifest (requires --download)")
    args = parser.parse_args()

    dict_path = Path(args.dict)
    phonetics_path = Path(args.phonetics)
    tags_path = Path(args.tags)
    out_path = Path(args.out)

    if args.download:
        download_inputs(args.cache_dir)
        dict_path = args.cache_dir / "ecdict.csv"
        phonetics_path = args.cache_dir / "output/phonetics.ddb"
        tags_path = args.cache_dir / "output/tag.ddb"
    if args.manifest_out and (not args.download or not args.overrides_out):
        parser.error("--manifest-out requires --download and --overrides-out")
    if out_path.resolve() in {dict_path.resolve(), phonetics_path.resolve(), tags_path.resolve()}:
        parser.error("output must not overwrite an input")

    if not dict_path.exists():
        sys.exit(f"missing dictionary CSV: {dict_path}")

    if not phonetics_path.exists() or not tags_path.exists():
        sys.exit("missing enrichment database; both phonetics.ddb and tag.ddb are required")
    phonetics = load_phonetics(phonetics_path)
    tags = load_tags(tags_path)
    print(
        f"loaded {len(phonetics):,} phonetics and {len(tags):,} tags",
        file=sys.stderr,
    )

    merged_phonetics = merged_tags = total = 0
    out_path.parent.mkdir(parents=True, exist_ok=True)
    temporary = out_path.with_suffix(out_path.suffix + ".part")
    with (
        dict_path.open(newline="", encoding="utf-8") as src,
        temporary.open("w", newline="", encoding="utf-8") as dst,
    ):
        reader = csv.reader(src)
        writer = csv.writer(dst)

        header = next(reader, None)
        if header != HEADER:
            sys.exit("unexpected dictionary CSV header")
        writer.writerow(header)

        for row in reader:
            if not row:
                continue
            if len(row) != len(HEADER):
                sys.exit(f"invalid CSV row {reader.line_num}")

            total += 1
            word = row[COL_WORD].strip().lower()

            phones = phonetics.get(word)
            if phones:
                phonetic = format_phonetics(*phones)
                if phonetic:
                    row[COL_PHONETIC] = phonetic
                    merged_phonetics += 1

            tag = tags.get(word)
            if tag:
                row[COL_TAG] = merge_tags(row[COL_TAG], tag)
                merged_tags += 1

            writer.writerow(row)

    os.replace(temporary, out_path)
    if args.overrides_out:
        overrides_count = write_overrides(args.overrides_out, phonetics, tags)
        if args.manifest_out:
            args.manifest_out.parent.mkdir(parents=True, exist_ok=True)
            args.manifest_out.write_text(json.dumps({
                "schema_version": 1,
                "revision": REVISION,
                "upstream": f"https://github.com/{UPSTREAM}",
                "base": {
                    "bytes": dict_path.stat().st_size,
                    "sha256": digest(dict_path),
                    "urls": list(download_urls("ecdict.csv")),
                },
                "enrichment": {
                    "file": args.overrides_out.name,
                    "bytes": args.overrides_out.stat().st_size,
                    "sha256": digest(args.overrides_out),
                    "rows": overrides_count,
                },
                "entries": total,
                "enriched_phonetics": merged_phonetics,
                "enriched_tags": merged_tags,
            }, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")

    print(
        f"wrote {out_path} ({total:,} entries; "
        f"{merged_phonetics:,} phonetics, {merged_tags:,} tags enriched)",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
