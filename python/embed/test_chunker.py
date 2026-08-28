import unittest

from chunker import chunk_text


class ChunkerTests(unittest.TestCase):
    def test_empty(self) -> None:
        self.assertEqual(chunk_text(""), [])

    def test_overlap_covers_boundary(self) -> None:
        text = "abcdefghij" * 20
        parts = chunk_text(text, max_chars=50, overlap=10)
        self.assertGreater(len(parts), 1)
        self.assertTrue(parts[1].startswith(parts[0][-10:]))

    def test_rejects_bad_overlap(self) -> None:
        with self.assertRaises(ValueError):
            chunk_text("hello", max_chars=32, overlap=32)


if __name__ == "__main__":
    unittest.main()
