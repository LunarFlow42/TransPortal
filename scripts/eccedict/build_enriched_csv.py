# /// script
# requires-python = ">=3.10"
# dependencies = ["duckdb"]
# ///
"""Build an enriched ECDICT CSV from the ECCEDICT fork's DuckDB data.

The ECCEDICT fork keeps its improvements (235k UK/US phonetics and 12.5k
refreshed exam tags) only in `output/phonetics.ddb` and `output/tag.ddb`, not in
`ecdict.csv`. This script merges them back into a single CSV that TransPortal
can import unchanged.

Usage (from a clone of https://github.com/H1DDENADM1N/ECCEDICT):

    uv run build_enriched_csv.py
    uv run build_enriched_csv.py --dict ecdict.csv --out ecdict.enriched.csv

Then in TransPortal: Settings -> Dictionary Sources -> Import CSV -> pick the
generated `ecdict.enriched.csv`.
"""

from __future__ import annotations

import argparse
import csv
import sys
from pathlib import Path

import duckdb

# Column indices in ecdict.csv / the enriched output.
COL_WORD = 0
COL_PHONETIC = 1
COL_TAG = 7


def load_phonetics(path: Path) -> dict[str, tuple[str | None, str | None]]:
    """word -> (uk, us) IPA from `output/phonetics.ddb` (DuckDB)."""
    con = duckdb.connect(str(path), read_only=True)
    try:
        rows = con.execute("SELECT word, phon_uk, phon_us FROM words").fetchall()
    finally:
        con.close()
    return {word: (uk, us) for word, uk, us in rows}


def load_tags(path: Path) -> dict[str, str]:
    """word -> space separated exam tags from `output/tag.ddb` (DuckDB)."""
    con = duckdb.connect(str(path), read_only=True)
    try:
        rows = con.execute("SELECT word, tag FROM words").fetchall()
    finally:
        con.close()
    return {word: tag for word, tag in rows if tag}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dict", default="ecdict.csv", help="source ecdict.csv")
    parser.add_argument("--phonetics", default="output/phonetics.ddb")
    parser.add_argument("--tags", default="output/tag.ddb")
    parser.add_argument("--out", default="ecdict.enriched.csv")
    args = parser.parse_args()

    dict_path = Path(args.dict)
    phonetics_path = Path(args.phonetics)
    tags_path = Path(args.tags)
    out_path = Path(args.out)

    if not dict_path.exists():
        sys.exit(f"missing dictionary CSV: {dict_path}")

    phonetics = load_phonetics(phonetics_path) if phonetics_path.exists() else {}
    tags = load_tags(tags_path) if tags_path.exists() else {}
    print(
        f"loaded {len(phonetics):,} phonetics and {len(tags):,} tags",
        file=sys.stderr,
    )

    merged_phonetics = merged_tags = total = 0
    with (
        dict_path.open(newline="", encoding="utf-8") as src,
        out_path.open("w", newline="", encoding="utf-8") as dst,
    ):
        reader = csv.reader(src)
        writer = csv.writer(dst)

        header = next(reader, None)
        if header is None:
            sys.exit("empty dictionary CSV")
        writer.writerow(header)

        for row in reader:
            if not row:
                continue

            total += 1
            word = row[COL_WORD]

            phones = phonetics.get(word)
            if phones:
                uk, us = phones
                parts = []
                if uk:
                    parts.append(f"英 {uk}")
                if us:
                    parts.append(f"美 {us}")
                if parts:
                    row[COL_PHONETIC] = " ".join(parts)
                    merged_phonetics += 1

            tag = tags.get(word)
            if tag:
                row[COL_TAG] = tag
                merged_tags += 1

            writer.writerow(row)

    print(
        f"wrote {out_path} ({total:,} entries; "
        f"{merged_phonetics:,} phonetics, {merged_tags:,} tags enriched)",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
