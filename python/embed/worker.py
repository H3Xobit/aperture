from chunker import chunk_text


def main() -> None:
    sample = (
        "Acme Q3 forecast. Industrial adhesives revenue up 12 percent. "
        "This worker only chunks text. Embeddings land in a later slice."
    )
    for i, part in enumerate(chunk_text(sample, max_chars=64, overlap=16)):
        print(f"{i}\t{part}")


if __name__ == "__main__":
    main()
