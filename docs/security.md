# Security and robustness guidance

Last updated: 2025-08-19

This document outlines safe usage patterns and robustness guidelines for storeit-rs. It focuses on preventing SQL injection, clarifying how builders and repositories use parameters, and providing a short checklist for production hardening.

## SQL injection safeguards

- Parameterized queries: All repository adapters and SQL builders in this workspace use parameter placeholders (`$1,$2,...` for Postgres; `?` for others). Values are passed separately from SQL strings and never interpolated into the SQL text.
- Identifiers (table/column names): Builders render identifiers that come from compile-time metadata (from the `#[derive(Entity)]` macro) or explicit method arguments (e.g., `select_by_field<E>("email")`).
  - The `Entity` macro validates identifiers (ASCII letters/digits/underscore, starting with a letter or `_`). This prevents generating obviously-invalid SQL from entity metadata.
  - For dynamic field names passed at runtime (e.g., `find_by_field(field_name, value)`), ensure `field_name` is derived from trusted sources (such as known entity metadata) and not from untrusted input. Do not accept arbitrary user strings as column names.
- Values: Never concatenate untrusted values into SQL strings. Always supply them via `ParamValue` to repository methods or via the builder’s returned SQL plus a parameter vector.

### Patterns to avoid

- Avoid building SQL with string concatenation of user-provided values:
  - Bad: `format!("... WHERE email = '{}'", user_input)`
  - Good: `repo.find_by_field("email", ParamValue::String(user_input)).await`.

### Patterns to prefer

- Use `#[repository(..., finders(find_by_email: String))]` to generate typed finders that internally call `find_by_field` with a known column name.
- Prefer builders like `select_by_field::<E>("email")` and pass the value separately as a parameter via your backend’s query function.

## Robustness checklist (quick)

- Forbid unsafe code in your crates unless you have a strong reason otherwise.
- Use short per-operation timeouts (100–1000 ms for OLTP) and keep transactions short.
- Handle transient errors with judicious retries on idempotent operations; use jittered exponential backoff. See docs/connection_pools.md for a helper sketch.
- Ensure migrations run before serving traffic; serialize runners to avoid races. See docs/migrations.md.
- Monitor your dependencies for advisories; enable CI scanning (cargo-audit) and review regularly.

## SQL builder invariants

The SQL builder functions only generate strings; they do not execute queries. Property-based tests (see storeit_sql_builder tests) assert basic invariants, such as:
- Placeholder counts match the number of parameters.
- For Postgres placeholder style, `$n` numbering is strictly increasing without gaps.
- Parentheses balance for compound WHERE clauses produced by AND/OR helpers.

These tests help guard against accidental regressions in SQL string generation.
