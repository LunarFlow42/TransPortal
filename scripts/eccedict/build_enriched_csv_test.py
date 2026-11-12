"""Development-time data builder tests (run with uv and duckdb)."""

import csv
import gzip
import hashlib
import importlib.util
import tempfile
import unittest
from pathlib import Path


spec = importlib.util.spec_from_file_location("build_enriched_csv", Path(__file__).with_name("build_enriched_csv.py"))
builder = importlib.util.module_from_spec(spec)
spec.loader.exec_module(builder)


class EnrichedDictionaryBuilderTests(unittest.TestCase):
    def test_public_pinned_downloads_prefer_ghproxy_and_keep_direct_fallbacks(self):
        urls = builder.download_urls("ecdict.csv")
        self.assertTrue(urls[0].startswith("https://gh-proxy.com/"))
        self.assertTrue(urls[1].startswith("https://ghproxy.net/"))
        self.assertTrue(urls[2].startswith("https://raw.githubusercontent.com/"))
        self.assertTrue(all(builder.REVISION in url for url in urls))

    def test_format_phonetics_normalizes_delimiters_and_missing_accents(self):
        self.assertEqual(builder.format_phonetics(" /həˈləʊ/ ", "/həˈloʊ/"), "英 /həˈləʊ/ 美 /həˈloʊ/")
        self.assertEqual(builder.format_phonetics(None, " /us/ "), "美 /us/")
        self.assertEqual(builder.format_phonetics("", None), "")

    def test_tags_are_unioned_not_replaced(self):
        self.assertEqual(builder.merge_tags("CET4 oxford ielts", "cet4 ky"), "cet4 ielts ky oxford")

    def test_pinned_input_verification_rejects_same_length_corruption(self):
        with tempfile.TemporaryDirectory(prefix="transportal-dictionary-builder-") as directory:
            path = Path(directory) / "input.csv"
            data = b"hello"
            path.write_bytes(data)
            blob = hashlib.sha1(b"blob 5\0" + data).hexdigest()
            self.assertTrue(builder.matches_input(path, blob, len(data)))
            path.write_bytes(b"world")
            self.assertFalse(builder.matches_input(path, blob, len(data)))

    def test_runtime_override_is_reproducible_sorted_and_contains_all_available_fields(self):
        with tempfile.TemporaryDirectory(prefix="transportal-dictionary-builder-") as directory:
            first = Path(directory) / "first.csv.gz"
            second = Path(directory) / "second.csv.gz"
            phonetics = {"world": ("/uk/", "/us/"), "hello": (None, "/hello/")}
            tags = {"hello": "cet4", "tagonly": "ielts"}
            self.assertEqual(builder.write_overrides(first, phonetics, tags), 3)
            builder.write_overrides(second, phonetics, tags)
            self.assertEqual(first.read_bytes(), second.read_bytes())
            with gzip.open(first, "rt", encoding="utf-8", newline="") as source:
                rows = list(csv.DictReader(source))
            self.assertEqual([row["word"] for row in rows], ["hello", "tagonly", "world"])
            self.assertEqual(rows[0]["phonetic"], "美 /hello/")
            self.assertEqual(rows[0]["tag"], "cet4")
            self.assertEqual(rows[2]["phonetic"], "英 /uk/ 美 /us/")


if __name__ == "__main__":
    unittest.main()
