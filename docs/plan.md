# Prioritized Plan (Concise)

Last updated: 2025-08-19

1. ~~Stabilize transactions across backends~~ ✓ (Aug 2025)
   - ~~Ensure repository reuse inside/outside transactions works uniformly; keep savepoint behavior consistent and documented.~~
2. ~~Tighten transaction mapping docs and ergonomics~~ ✓ (Aug 2025)
   - ~~Keep docs/transactions.md authoritative; expose simple defaults via TransactionTemplate and make read-only/timeout semantics clear.~~
3. ~~Strengthen real‑DB integration coverage (CI)~~ ✓ (Aug 2025)
   - ~~Maintain scheduled/container jobs; keep default runs fast; ensure parity tests pass for PG/MySQL/libsql.~~
4. ~~Improve developer ergonomics in macros~~ ✓ (Aug 2025)
   - ~~Better diagnostics for unsupported types/attributes; add trybuild tests for common mistakes (short messages).~~
5. ~~Expand SQL builder helpers prudently~~ ✓ (Aug 2025)
   - ~~Keep builder minimal; provide only high‑value helpers (e.g., IS NULL/NOT NULL, IN/NOT IN, simple ordering/pagination already done).~~
6. ~~Documentation consolidation and examples~~ ✓ (Aug 2025)
   - ~~Keep this plan and Architecture as single sources; ensure README example mirrors the canonical approach.~~
7. ~~Optional: libsql RETURNING opt‑in kept minimal~~ ✓ (Aug 2025)
   - ~~Keep feature flag for RETURNING; default to portable last_insert_rowid.~~
8. ~~Optional: small performance footguns~~ ✓ (Aug 2025)
   - ~~Retain per‑repo SQL caching; avoid per‑call string construction and unnecessary allocations.~~

9. ~~License and contribution basics before publish~~ ✓ (Aug 2025)
   - ~~Add LICENSE (Apache-2.0 or MIT/Apache-2 dual) and CONTRIBUTING.md; include CLA note if needed.~~
   - ~~Add CODE_OF_CONDUCT.md referencing the Contributor Covenant.~~
10. ~~CI hardening and lint policy~~ ✓ (Aug 2025)
   - ~~Enforce cargo clippy --workspace --all-features -Dwarnings in CI (matrix: stable + MSRV).~~
   - ~~Add link check for docs and README; run cargo doc --all-features -D warnings.~~
   - ~~Add feature-combinations build matrix for facade crate (each backend feature + none).~~
11. ~~Repository ergonomics and examples refresh~~ ✓ (Aug 2025)
   - ~~Provide minimal end-to-end example per backend in examples/ with migrations and a short README.~~
   - ~~Add a quick-start template (cargo-generate friendly) showing Entity + #[repository] + TransactionTemplate usage.~~
12. ~~Query capabilities: pagination and counts~~ ✓ (Aug 2025)
   - ~~Add builder helper select_count_by_field/select_count_all for consistent total counts.~~
   - ~~Provide optional RepositoryExt trait with paginate(page,size) returning {items,total}.~~
13. ~~Batching and streaming (opt-in)~~ ✓ (Aug 2025)
   - ~~Add simple batch insert API (insert_many) behind optional feature; initial implementation uses a naive loop. Prepared `storeit_sql_builder::insert_many` for multi-row VALUES across backends.~~
   - ~~Provide an opt-in streaming helper `find_by_field_stream` that yields a Stream by wrapping `find_by_field`; backends can later optimize.~~
14. ~~Error taxonomy and mapping consistency~~ ✓ (Aug 2025)
   - ~~Document mapping of common backend errors into storeit_core::RepoError; add tests ensuring Mapping vs Backend boundaries.~~
   - ~~Provide context-rich messages (table/column) in RowAdapter errors via macros where possible.~~
15. ~~Type coverage and portability~~ ✓ (Aug 2025)
   - ~~Extend ParamValue and derive support for common types: chrono::NaiveDateTime/Date, decimal (rust_decimal), uuid (feature-gated per backend).~~
   - ~~Add basic unit tests covering insert/update value extraction for these types (portable string mapping). Cross‑backend finders for these types can be added later.~~
16. ~~Performance baseline and benches~~ ✓ (Aug 2025)
   - ~~Add criterion benchmarks for insert/find/update/delete across backends (skip container benches by default).~~
   - ~~Track simple perf budgets and regressions in CI via a lightweight trend job (non-blocking).~~

17. ~~API stabilization pre-1.0~~ ✓ (Aug 2025)
   - ~~Define a public API audit checklist (macros, traits, feature flags) and freeze breaking changes behind explicit feature gates.~~
   - ~~Establish deprecation policy and semver contract; document MSRV and support window.~~
18. ~~Macro ergonomics & diagnostics~~ ✓ (Aug 2025)
   - ~~Improve compile-time error messages with actionable hints and links to docs; add more trybuild cases for common mistakes.~~
   - ~~Add attribute niceties: #[fetch(skip)] for non-persistent fields; table/column rename strategies and validation.~~
19. ~~Upsert and conflict handling (backend-aware)~~ ✓ (Aug 2025)
   - ~~SQL builder: helpers for Postgres ON CONFLICT, MySQL ON DUPLICATE KEY UPDATE, and a portable fallback doc.~~
   - ~~Facade: opt-in trait methods (feature-gated) delegating to backend-specific paths when available.~~
20. ~~Observability: tracing and metrics (opt-in)~~ ✓ (Aug 2025)
   - ~~Add tracing spans around repo calls (feature = "tracing") with fields: sql_kind, table, op, rows, elapsed.~~
   - ~~Optional metrics integration (feature = "metrics"): counters/histograms for ops and errors via a small trait shim.~~
21. ~~Connection management and pooling guidance~~ ✓ (Aug 2025)
   - ~~Provide recommended pool configurations and timeouts for each backend; add health-check helper examples.~~
   - ~~Document retry/backoff patterns for transient errors; provide a simple retry wrapper example.~~
22. ~~Migrations integration examples~~ ✓ (Aug 2025)
   - ~~Add minimal examples integrating with popular tools (refinery, sqlx migrate) and a tiny MigrationRunner trait sketch in docs.~~
   - ~~Ensure examples avoid race conditions and clearly state ordering/transactionality expectations.~~
23. ~~Query ergonomics: where-builder and keyset pagination (opt-in)~~ ✓ (Aug 2025)
   - ~~Introduce a tiny, explicit where-clause builder for simple AND/OR trees that renders to SQL + params.~~
   - ~~Provide a keyset pagination helper alongside existing LIMIT/OFFSET guidance.~~
24. ~~Backend parity tests and conformance~~ ✓ (Aug 2025)
   - ~~Expand parity tests to assert behavior and error mapping consistency across PG/MySQL/LibSQL for CRUD and finders.~~
   - ~~Add an adapter conformance checklist and run it behind containerized CI.~~
25. ~~Security & robustness hardening~~ ✓ (Aug 2025)
   - ~~Document SQL injection safeguards and usage patterns; fuzz tests for SQL builder string generation.~~
   - ~~Lint policy: consider forbidding unsafe_code; audit dependencies and enable supply-chain scanning in CI.~~
26. ~~Cross-platform CI & platform notes~~ ✓ (Aug 2025)
   - ~~Add Windows and macOS jobs for core crates and SQL builder; document any platform-specific caveats.~~
27. ~~Documentation site & guides~~ ✓ (Aug 2025)
   - ~~Publish mdBook or MkDocs site (hosted via GitHub Pages); add guides for common tasks and troubleshooting.~~
28. ~~Release automation~~ ✓ (Aug 2025)
   - ~~Add cargo-release configuration, CHANGELOG policy (Keep a Changelog), and tagging workflow; optional conventional commits.~~

29. ~~Documentation and crate naming consistency~~ ✓ (Aug 2025)
   - ~~Align names across docs to the actual crates: storeit (facade), storeit_core, storeit_macros. Add an explicit note about optionally aliasing the facade as `repository` in Cargo.toml (e.g., `repository = { package = "storeit", path = "./storeit" }`).~~
   - ~~Replace remaining references to repo_core/repo_macros and the non-existent `./repository` path. Ensure code snippets compile with either `storeit::...` or with the documented alias.~~ 

30. ~~README dependency and feature guidance~~ ✓ (Aug 2025)
   - ~~Fix dependency examples to use the correct path/package: `repository = { package = "storeit", path = "./storeit", features = ["libsql-backend"] }` (and similar for postgres-backend/mysql-async), or show crates.io usage.~~
   - ~~Audit all feature names in README and ensure they match the facade crate’s features: libsql-backend, postgres-backend, mysql-async.~~

31. ~~Architecture doc crate names~~ ✓ (Aug 2025)
   - ~~Update docs/architecture.md to use storeit_core/storeit_macros naming (or clearly explain the aliasing convention if keeping `repository` as a facade name in examples).~~

32. ~~mdBook and docs aliasing clarification~~ ✓ (Aug 2025)
   - ~~In docs/book Quick Start and other docs using `repository::...`, add an introductory note explaining dependency renaming via Cargo.toml so examples line up with `repository::` paths.~~

33. ~~Makefile integration-backends target~~ ✓ (Aug 2025)
   - ~~The target line is indented; move `integration-backends:` to column 0 and add it to .PHONY. This currently risks make ignoring or mis-parsing the target.~~

34. ~~CI workflows presence vs. plan~~ ✓ (Aug 2025)
   - ~~Add missing GitHub Actions workflows to match stated policy: clippy (deny warnings), fmt check, tests (stable + MSRV matrix), docs build (deny warnings), and a link checker for README/docs. Include a simple feature-matrix build for the facade crate (no backend, each backend feature individually).~~

35. ~~MSRV consistency~~ ✓ (Aug 2025)
   - ~~Add `rust-version = "1.70"` to backend crates that lack it (storeit_libsql, storeit_mysql_async, storeit_tokio_postgres) to align with the workspace policy mentioned in README.~~

36. ~~Transactions docs: IS NULL ergonomics~~ ✓ (Aug 2025)
   - ~~Transactions doc notes that `find_by_field` with `ParamValue::Null` doesn’t match NULL rows. Plan: add explicit `select_by_is_null`/`select_by_is_not_null` helpers to the SQL builder and surface finder helpers or guidance in the facade. Ensure tests exist across backends.~~

37. Remove `anyhow` from the workspace (plan)
   - Goal: eliminate `anyhow` as a dependency from all workspace crates (prod and dev/tests) and examples. Keep a single unified error type: `storeit_core::RepoError` with `RepoResult<T>`.
   - Phase 1 — Code & Manifests (public/backends):
     - ~~Remove `anyhow` from backend crates Cargo.toml: `storeit_libsql`, `storeit_mysql_async`, `storeit_tokio_postgres`. Replace any remaining `anyhow::{bail!, Context}` with `RepoError::{backend,mapping}` and explicit error values.~~ ✓
     - ~~Ensure all adapters and transaction managers map driver errors via `RepoError::backend` and row conversions via `RepoError::mapping`.~~ ✓
   - Phase 2 — Examples and facade dev-deps:
     - ~~Remove `anyhow` from `storeit/Cargo.toml` dev-dependencies.~~ ✓
     - ~~Change example mains to return `Result<(), storeit::RepoError>` and adjust error uses accordingly.~~ ✓
   - Phase 3 — Test crates and integration harness:
     - ~~`tests_common`: drop `anyhow` dependency; change `RepoFactory::new_user_repo` to return `storeit_core::RepoResult<Box<dyn Repository<User> + Send + Sync>>` and update call sites in backend integration tests.~~ ✓
     - ~~Update backend integration tests to use `RepoResult` (replace `anyhow::Context` with driver error mapping or explicit messages). Ensure skip/containers logic still compiles.~~ ✓
   - Phase 4 — Builder and macros dev-deps:
     - ~~Remove `anyhow` from `storeit_sql_builder` dev-deps; adjust tests to use `Result<_, Box<dyn std::error::Error>>` or plain `Result<(), ()>` as needed.~~ ✓
     - ~~Remove `anyhow` from `storeit_macros` dev-deps (trybuild tests typically don’t need it). Ensure macro-generated code never references `anyhow` (already migrated to `RepoError`).~~ ✓
   - Phase 5 — Documentation and guidance:
     - ~~Replace all occurrences of `anyhow::Result` in README, docs/architecture.md, docs/book, and guides with `storeit::RepoResult` (or `storeit_core::RepoResult`) in snippets. Use `storeit::RepoError` in function signatures where needed.~~ ✓
     - ~~Add a short migration note: “0.x → 0.x: Repository APIs use RepoError/RepoResult; remove anyhow, map driver errors via RepoError::backend, mapping via RepoError::mapping.”~~ ✓
   - Phase 6 — CI safety net:
     - ~~Add a quick CI job/step to fail if `anyhow` appears in the dependency tree (e.g., `cargo tree -i anyhow` or `grep -R "anyhow\b"` over Cargo.toml). Optional: add cargo-deny policy to ban `anyhow`.~~ ✓
   - Acceptance criteria:
     - `cargo test --workspace --all-features` passes.
     - No direct `anyhow` in workspace manifests (prod or dev/tests); our code avoids `anyhow` entirely. Third-party crates may still depend on `anyhow`.
     - All docs/examples compile (doctests/examples) without `anyhow` in our code.
