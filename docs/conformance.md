# Adapter Conformance and Backend Parity

Last updated: 2025-08-19

This document defines a lightweight checklist that every backend adapter in this workspace (and third-party adapters) should satisfy. It also describes the shared parity tests we run across backends to keep behavior consistent.

## Scope

- Adapters in-tree: storeit_libsql (SQLite/libsql), storeit_mysql_async (MySQL), storeit_tokio_postgres (Postgres).
- Third-party adapters: recommended to mirror this checklist and provide a small test harness that reuses tests_common where possible.

## Conformance checklist

1) Construction
- [ ] Provide a repository type `BackendRepository<T, A>` with async constructors:
- [ ] `from_url(&str, adapter)` (or equivalent pool/builder) returning a ready-to-use repo.
- [ ] Document required feature flag(s) on the crate to enable the backend.

2) CRUD and Visibility
- [ ] `insert(&T) -> T` returns a materialized entity (id populated); if the backend supports RETURNING, use it; otherwise fetch by last insert id using the same connection.
- [ ] `find_by_id(&K) -> Option<T>` returns None when missing and Some(T) when present.
- [ ] `update(&T) -> T` round-trips the input entity (reflect server-modified columns if any) and does not silently drop errors.
- [ ] `delete_by_id(&K) -> bool` returns true only when a row was affected.
- [ ] After `insert`, a subsequent `find_by_id` or `find_by_field` sees the row using the same connection/transaction context.

3) Generic finder parity
- [ ] `find_by_field(field, ParamValue)` matches by equality and returns zero or more rows.
- [ ] Semantics for NULL equality match SQL behavior (e.g., `= NULL` yields no rows) and tests document this.
- [ ] Type mismatches (e.g., passing ParamValue::I32 for a TEXT column) surface as backend errors.

4) RowAdapter mapping
- [ ] Mapping failures from rows to entities propagate as errors (e.g., missing columns or bad indices).
- [ ] Error messages include helpful context (table/column where applicable).

5) Transactions (best-effort parity)
- [ ] When a transaction is active, repository methods prefer the transaction-bound connection/handle from task-local storage.
- [ ] Begin/commit/rollback and savepoints work per backend capabilities; read-only/timeouts honored best-effort.

6) Observability (opt-in)
- [ ] With feature `tracing`, span or event fields include: sql_kind="sql", table, op, rows, elapsed_ms, success.
- [ ] With feature `metrics`, counters/histograms are recorded: repo_ops_total, repo_op_errors_total, repo_op_duration_ms.

## Parity tests

A small suite of generic tests lives in `tests_common` and is reused by backend-specific harnesses in each adapter crate’s `tests/` folder. The suite covers:
- CRUD roundtrip (insert/find/update/delete)
- Generic finder by field (equality)
- Behavior of find_by_field with boolean and NULL
- Surface of duplicate key/unique constraint errors
- RowAdapter mapping failures surfacing as errors
- Parameter type mismatch errors

See:
- storeit_tokio_postgres/tests/integration_postgres.rs
- storeit_mysql_async/tests/integration_mysql.rs
- storeit_libsql/tests/integration_libsql.rs

These tests use `testcontainers` and are marked `#[ignore]` by default to keep normal CI fast. A dedicated workflow runs them on schedule and on demand.

## How to add a new backend

- Implement the checklist above.
- Create a `tests/` harness that uses `tests_common` entity and migrations, reproducing the parity tests.
- Ensure container images are lightweight and pulled from public registries.
- Add the backend to the integration-backends workflow matrix when it’s ready.

