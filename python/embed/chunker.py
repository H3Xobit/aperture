"""Split source text into overlapping chunks for the embed worker."""

from __future__ import annotations


def chunk_text(text: str, max_chars: int = 512, overlap: int = 64) -> list[str]:
    body = " ".join(text.split())
    if not body:
        return []
    if max_chars < 32:
        raise ValueError("max_chars must be at least 32")
    if overlap < 0 or overlap >= max_chars:
        raise ValueError("overlap must be in [0, max_chars)")
    chunks: list[str] = []
    start = 0
    n = len(body)
    while start < n:
        end = min(n, start + max_chars)
        chunks.append(body[start:end])
        if end >= n:
            break
        start = end - overlap
    return chunks
