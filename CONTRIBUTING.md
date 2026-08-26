# Contributing to Vestige

Thank you for your interest in contributing to Vestige! This guide covers everything you need to get started.

## Project Overview

Vestige is a cognitive memory MCP server written in Rust. It gives AI agents persistent long-term memory using neuroscience-backed algorithms (FSRS-6, prediction error gating, synaptic tagging, spreading activation, memory dreaming).

**Architecture:**

```
vestige/
├── crates/
│   ├── vestige-core/       # Cognitive engine, FSRS-6, search, embeddings, storage
│   └── vestige-mcp/        # MCP server, Axum dashboard, WebSocket, tool handlers
├── apps/
│   └── dashboard/          # SvelteKit + Three.js 3D dashboard
├── packages/
│   ├── vestige-init/       # npx @vestige/init installer
│   └── vestige-mcp-npm/    # npm binary wrapper
└── tests/
    └── vestige-e2e-tests/  # End-to-end MCP protocol tests
```

## Development Setup

### Prerequisites

- **Rust** (1.85+ stable): [rustup.rs](https://rustup.rs)
- **Node.js** (v22+): [nodejs.org](https://nodejs.org)
- **pnpm** (v9+): `npm install -g pnpm`

### Getting Started

```bash
git clone https://github.com/samvallad33/vestige.git
cd vestige

# Build the dashboard (required for include_dir! embedding)
cd apps/dashboard && pnpm install && pnpm build && cd ../..

# Build the Rust workspace
cargo build

# Run tests
cargo test --workspace
```

> **Heads up: tests use the real embedding model.** There is no mock-embeddings
> switch. Any test that ingests a memory goes through `Storage::ingest` →
> `EmbeddingService::is_ready()`, which initializes fastembed's
> `nomic-embed-text-v1.5` and downloads ~522 MB into the cache directory on a
> cold run. If the download fails, `is_ready()` returns `false` and the tests
> still pass — with embeddings silently disabled, after a ~30 s stall. Set
> `FASTEMBED_CACHE_PATH` to a persistent directory to avoid re-downloading.

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `FASTEMBED_CACHE_PATH` | Where the fastembed ONNX model is cached. Defaults to the platform cache dir (Linux: `~/.cache/core/fastembed`) |
| `VESTIGE_RETENTION_TARGET` | Retention threshold used by health reporting (default `0.8`) |
| `VESTIGE_CONSOLIDATION_INTERVAL_HOURS` | Background consolidation interval (default `6`) |
| `VESTIGE_DASHBOARD_PORT` | Dashboard HTTP port (default `3927`) |
| `VESTIGE_ENCRYPTION_KEY` | SQLCipher key. Only read when built with `--features encryption` |
| `RUST_LOG` | Log level filter (e.g. `debug`, `info`, `warn`) |

The database path is **not** environment-configurable — use the
`--data-dir <PATH>` flag on `vestige-mcp`.

## Running Tests

```bash
# All tests (734 total)
cargo test --workspace

# Core library tests only (352 tests)
cargo test -p vestige-core --lib

# MCP server tests only (378 tests).
# `--bin vestige-mcp` is load-bearing: lib.rs exposes only `cognitive` and
# `dashboard`, so `--lib` alone reports "0 passed" and exits 0.
cargo test -p vestige-mcp --bin vestige-mcp

# E2E MCP protocol tests (requires release build)
cargo build --release -p vestige-mcp
cargo test -p vestige-e2e-tests --test mcp_protocol -- --test-threads=1

# Dashboard build test
cd apps/dashboard && pnpm build
```

## Building

```bash
# Debug build
cargo build -p vestige-mcp

# Release build (22MB binary with embedded dashboard)
cargo build --release -p vestige-mcp

# The release binary is at target/release/vestige-mcp
```

### Release Profile

The release profile uses `lto = true`, `codegen-units = 1`, `opt-level = "z"`, and `strip = true` for minimum binary size.

## Code Style

### Rust

```bash
# Format
cargo fmt --all

# Lint (zero warnings policy)
cargo clippy --workspace -- -D warnings
```

- Rust 2024 edition
- Standard `rustfmt` defaults
- All public items should have doc comments
- Tests go in `#[cfg(test)] mod tests` at the bottom of each file

### TypeScript/Svelte (Dashboard)

```bash
cd apps/dashboard
pnpm check    # Svelte type checking
pnpm lint     # ESLint
```

## Project Structure

### vestige-core

The cognitive engine. Key modules:

| Module | Purpose |
|--------|---------|
| `fsrs/` | FSRS-6 spaced repetition (21 parameters, power-law decay) |
| `neuroscience/` | Synaptic tagging, spreading activation, hippocampal index, importance signals |
| `advanced/` | Prediction error gating, dreaming, compression, cross-project learning |
| `search/` | Hybrid search (BM25 + semantic), HyDE, reranker, temporal search |
| `embeddings/` | fastembed (Nomic Embed v1.5), ONNX inference |
| `storage/` | SQLite + FTS5 + USearch HNSW |

### vestige-mcp

The MCP server and dashboard. Key modules:

| Module | Purpose |
|--------|---------|
| `server.rs` | MCP JSON-RPC server (rmcp 0.14) |
| `cognitive.rs` | CognitiveEngine — 29 stateful modules |
| `tools/` | One file per MCP tool (21 tools) |
| `dashboard/` | Axum HTTP + WebSocket + event bus |

### apps/dashboard

SvelteKit 2 + Three.js + Tailwind CSS. Pages:

- `/dashboard` — 3D memory graph with force-directed layout
- `/dashboard/memories` — Searchable memory browser
- `/dashboard/timeline` — Chronological memory timeline
- `/dashboard/feed` — Real-time WebSocket event stream
- `/dashboard/explore` — Connection explorer (associations, chains, bridges)
- `/dashboard/intentions` — Intention manager
- `/dashboard/stats` — System health, retention distribution, module status

## Pull Request Process

1. **Fork** the repository and create a feature branch from `main`
2. **Write tests** for new functionality
3. **Ensure all checks pass**: `cargo fmt`, `cargo clippy`, `cargo test`
4. **Build the dashboard** if you modified `apps/dashboard/`
5. **Keep commits focused**: One logical change per commit
6. **Open a PR** with a clear description

### PR Checklist

- [ ] `cargo fmt --all` — code is formatted
- [ ] `cargo clippy --workspace -- -D warnings` — zero warnings
- [ ] `cargo test --workspace` — all tests pass
- [ ] Dashboard builds (if modified): `cd apps/dashboard && pnpm build`
- [ ] No secrets, API keys, or credentials in code

### Good First Issues

Look for issues labeled `good first issue`. These are scoped, well-defined tasks ideal for new contributors:

- Adding tests for existing modules
- Documentation improvements
- Dashboard UI enhancements
- New MCP tool implementations

## Adding a New MCP Tool

1. Create `crates/vestige-mcp/src/tools/your_tool.rs`
2. Implement `pub fn schema() -> Tool` and `pub fn execute(...) -> Result<CallToolResult>`
3. Register in `crates/vestige-mcp/src/tools/mod.rs`
4. Add tests in the same file
5. Update tool count in README and CLAUDE.md

## Adding a New Cognitive Module

1. Add the module to `crates/vestige-core/src/neuroscience/` or `advanced/`
2. Add the field to `CognitiveEngine` in `crates/vestige-mcp/src/cognitive.rs`
3. Initialize it in `CognitiveEngine::new()` and `new_with_events()`
4. Write comprehensive tests (aim for 10+ per module)
5. Document the neuroscience citation in the module's doc comment

## Issue Reporting

Use the issue templates:

- **Bug Report**: Include OS, install method, IDE, vestige version, and steps to reproduce
- **Feature Request**: Describe the problem, proposed solution, and alternatives considered

## Code of Conduct

We are committed to providing a welcoming and inclusive environment. All contributors are expected to be respectful, constructive, and collaborative. Harassment and discrimination will not be tolerated.

## License

By contributing, you agree that your contributions will be licensed under **AGPL-3.0-only** ([LICENSE](LICENSE)), the same license as the project.

---

Questions? Open a [discussion](https://github.com/samvallad33/vestige/discussions) or reach out to the maintainers.
