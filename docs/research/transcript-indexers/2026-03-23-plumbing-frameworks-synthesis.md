# RAG Plumbing Frameworks Synthesis

This document synthesizes findings on integrating specialized plumbing frameworks (`swiftide`, `cocoindex`, and `RustyRAG`) into a local-first agent memory tool (like `transcript-browser`).

## 1. Swiftide ( bosun-ai/swiftide )

**Overview:**
Swiftide is a fast, streaming indexing and query pipeline framework for Rust. It uses an asynchronous stream-based architecture (`tokio` and `futures_util`) to define pipelines that ingest, transform, embed, and store documents.

**Relevance to Agent Memory:**
Swiftide is highly suited for the "plumbing" of an agent memory system. It allows developers to offload the complexities of concurrency, batching, error recovery, and stream processing.

**Key Abstractions:**
- **`Pipeline`**: A builder pattern to compose a stream. `swiftide_indexing::Pipeline` manages the flow of `Node`s.
- **`Node`**: The fundamental unit of data flowing through the pipeline. It contains the data `Chunk`, optional `vectors`, `sparse_vectors`, and `metadata`.
- **`Loader`**: A trait to implement custom data sources.

**Implementing a `ClaudeLogLoader`:**
Implementing a custom loader for Claude Code JSONL logs is straightforward. You implement the `Loader` trait:

```rust
pub trait Loader: DynClone + Send + Sync {
    type Output: Chunk;
    fn into_stream(self) -> IndexingStream<Self::Output>;
}
```

You would create a struct `ClaudeLogLoader` that takes a directory path, uses `tokio::fs` or `ignore::WalkBuilder` to find `.jsonl` files, reads them, parses the JSON (perhaps using `simd-json` or standard `serde_json`), and yields `Node` objects representing the conversation turns into the `IndexingStream`.

**Pros:**
- **Robustness:** Handles backpressure, retries, and batching automatically.
- **Composability:** You can easily add intermediate steps like `then_chunk` or `then_embed` before sending to storage.

**Cons:**
- It is primarily a pipeline execution framework. You still need to provide the storage backend (e.g., Qdrant, LanceDB, or an embedded DB like VelesDB).

## 2. Cocoindex ( cocoindex-io/cocoindex )

**Overview:**
Cocoindex bills itself as a data transformation framework for AI, emphasizing "incremental processing".

**Relevance to Agent Memory:**
Cocoindex's strength lies in its ability to track state. For an application that continuously watches a directory of logs, you only want to process *new* lines or *modified* files to avoid re-embedding existing text, which is computationally expensive.

**Key Abstractions:**
Based on a brief inspection of its Rust source structure, it has a complex schema and execution engine (`schema.rs`, `spec.rs`, `execution/`, `ops/`). It appears to be designed as a heavier ETL framework, possibly with Python bindings acting as a significant part of its user interface.

**Pros:**
- Incremental indexing is built-in, solving a major headache for local log watchers.

**Cons:**
- Appears to have a steeper learning curve and a more complex internal architecture compared to Swiftide's straightforward asynchronous streams. It might be overkill for simply parsing JSONL files.

## 3. RustyRAG ( AlphaCorp-AI/RustyRAG )

**Overview:**
RustyRAG is a production-grade RAG application framework in Rust, focusing on hybrid search (HNSW + BM25) and integrating various AI services (Milvus, Jina AI, Groq).

**Relevance to Agent Memory:**
While not a generalized "plumbing" library like Swiftide, RustyRAG serves as an excellent reference implementation. It demonstrates how to wire up an Actix Web server with Milvus vector storage and text rerankers.

**Pros:**
- Excellent blueprint for configuring high-quality hybrid search.

**Cons:**
- It is a standalone application template rather than a modular crate you can easily import to build a local CLI. It relies on external network services like Milvus and Docling rather than embedded files.

## Summary

If building a custom local memory controller:
1.  **Swiftide** offers the most ergonomic Rust API for building the ingestion pipeline. Writing a `Loader` for Claude logs returning a stream of `Node`s is a highly idiomatic Rust pattern. It is the recommended choice for "plumbing".
2.  **Cocoindex** offers powerful incremental state tracking, but its architecture is more opaque and ETL-focused, potentially making it heavier to embed purely as a library in a small CLI tool.

For the `transcript-browser` project, the most robust path forward is utilizing **Swiftide** for ingestion plumbing, paired with a reliable, production-grade embedded storage backend like **LanceDB**, **SQLite (FTS5)**, or **Tantivy** for full-text and vector search, avoiding unproven "all-in-one" AI database wrappers.