# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog (https://keepachangelog.com/en/1.1.0/),
and this project adheres to Semantic Versioning (https://semver.org/spec/v2.0.0.html).

## [Unreleased]

- Planned: stabilize APIs under `unstable` feature gates before 1.0.
- Planned: expand backend observability parity.

## [0.1.0] - 2025-08-19

### Added
- Initial workspace layout with core traits (storeit_core), macros (storeit_macros), facade (storeit), SQL builder (storeit_sql_builder), and backends for libsql, MySQL (storeit_mysql_async), and Postgres (storeit_tokio_postgres).
- Transaction templates and managers with best-effort parity across backends.
- Examples for each backend under the repository crate.
- CI hardening: format, clippy, tests, docs build, link check; cross-platform matrix for core/builder.
- Security docs, connection pooling/migrations guides, and adapter conformance checklist.
- Benchmarks for libsql (Criterion) and non-blocking bench workflow.
- Optional extensions (feature-gated): query-ext, batch-ext, stream-ext, upsert-ext.
- Observability (opt-in): tracing/metrics instrumentation for libsql backend.

### Changed
- Improved macro diagnostics, identifier validation, and mapping error context.

### Fixed
- Transaction-bound visibility after insert for libsql (fetch same connection).
- tokio_postgres transaction timeout setting corrected to use `SET LOCAL statement_timeout = '<ms>ms'`.

