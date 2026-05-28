# Graphon — Project Context for BMAD

## Project Overview

Graphon is a **high-performance email indexing, automated sorting, and cleaning tool** written in Rust, designed for Gmail. It processes emails and attachments, applies intelligent labels, and prepares clean chunks of text for ingestion into a **RAG (Retrieval-Augmented Generation)** knowledge base.

## Architecture

### Backend — Hexagonal / Clean Architecture (Rust)

4-crate Cargo workspace:

| Crate | Role |
|---|---|
| `graphon-core` | Domain layer — pure entities (`Email`, `Attachment`, `Label`), ports (traits like `GmailPort`, `StoragePort`, `ClassifierPort`), central error handling, no I/O |
| `graphon-infrastructure` | Adapter implementations (Gmail API integration, PostgreSQL/SQLx persistence, LLM/heuristic email classification, RAG export) |
| `graphon-application` | Use cases: email sorting pipeline, RAG ingestion/indexing flow, retention-based garbage collection |
| `graphon-server` | Entrypoints: CLI command engine for batch jobs / cron execution, and Axum HTTP server for webhook events (Gmail Pub/Sub) and metrics |

## Tech Stack

| Layer | Technology |
|---|---|
| Language (backend) | Rust 2021 edition |
| Async runtime | Tokio 1.36+ |
| HTTP framework | Axum 0.7+ |
| Serialization | Serde / serde_json |
| Database / Persistance | PostgreSQL + SQLx |
| Error handling | anyhow + thiserror |
| Observability | Prometheus + tracing |
| Google Integration | google-gmail1 (Gmail API) + OAuth2 |

## Key Patterns

- **Hexagonal Ports & Adapters**: Complete decoupling of Google API interactions and database storage from the core domain logic.
- **Email Sorting Pipeline**: Ingests unread messages, classifies them based on rules or AI model predictions, updates Gmail labels accordingly, and indexes relevant content.
- **Garbage Collection / Retention Rules**: Automatically deletes or archives expired notifications, OTP codes, or obsolete alert emails.
- **RAG Preprocessing**: Parses email text and documents, creates clean text chunks, and structures them ready for vector embedding insertion.

## Development Conventions

- Run all tests: `cargo test`
- Format: `cargo fmt`
- Lint: `cargo clippy -- -D warnings`
- Build: `cargo build --release`
