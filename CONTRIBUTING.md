# Contributing to storeit-rs

Thanks for your interest in contributing! This document describes how to get started and the conventions we follow.

## Repository layout
- Workspace crates:
  - storeit_core, storeit_macros, storeit (facade), storeit_sql_builder
  - Backends (feature-gated crates): storeit_libsql, storeit_mysql_async, storeit_tokio_postgres
- Docs: ./docs (architecture, transactions, plan)

## Prerequisites
- Rust toolchain (stable). Recommended: rustup with the latest stable.
- Optional for coverage: `rustup component add llvm-tools-preview` and `cargo install cargo-llvm-cov`.
- Optional for integration tests with containers: Docker.

## Build and test
- Build everything: `cargo build`
- Run unit tests and doc tests quickly: `cargo test`
- Run backend integration tests in skip mode (fast): `make integration-backends`
- Run with containers (real DBs): `RUN_CONTAINERS=1 make integration-backends`
- Coverage summary: `make coverage-all-summary` (add `RUN_CONTAINERS=1` to include container tests)

## Lints and formatting
- Format: `cargo fmt --all`
- Clippy (strict): `cargo clippy --workspace --all-features -- -Dwarnings`

## Submitting changes
1. Fork the repo and create your branch from `main`.
2. Add tests for your change where applicable.
3. Ensure `cargo test` and `cargo clippy --workspace --all-features -- -Dwarnings` pass.
4. Open a Pull Request with a clear description of the change and rationale.

### Commit sign-off (DCO)
We use the [Developer Certificate of Origin](https://developercertificate.org/) (DCO) instead of a CLA. Each commit must be signed off to certify that you wrote the code or have the right to submit it. Use:

```
git commit -s -m "Your commit message"
```

The sign-off line will appear as: `Signed-off-by: Your Name <you@example.com>`

### Licensing
By contributing, you agree that your contributions will be dual-licensed under the terms of both the MIT license and the Apache License, Version 2.0, as specified in the repository LICENSE file.

## Code of Conduct
Participation in this project is governed by our [Code of Conduct](./CODE_OF_CONDUCT.md).
