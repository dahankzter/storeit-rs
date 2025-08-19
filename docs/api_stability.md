# API Stability, SemVer, and Support Policy (pre-1.0)

Last updated: 2025-08-19

This document describes how we stabilize the public API surface of the storeit-rs workspace on the path to 1.0. It applies to all crates in this repository.

Scope of “public API” here includes:
- storeit_core: traits, type aliases, error types, transactions API.
- storeit_macros: proc-macros (#[derive(Entity)], #[repository(...)]), their accepted attributes and generated items.
- storeit (facade): facade re-exports, optional extension traits behind features, re-exported backends and transactions.
- storeit_sql_builder: public builder functions and enums.
- Backend crates (storeit_libsql, storeit_mysql_async, storeit_tokio_postgres): Repository types, TransactionManager types, and their public constructors.


## SemVer contract

- Until 1.0.0: We may introduce breaking changes, but we aim to minimize them and gate experimental/breaking additions behind an explicit feature flag named `unstable`.
- After 1.0.0: We follow SemVer strictly. Any change that can break downstream code requires a major version bump.
- Patch versions must only include bug fixes or strictly additive changes (no breaking signature changes, no new required trait bounds).
- Minor versions may add new APIs in a backward-compatible way (new functions, optional traits, new features that are off by default, etc.).


## Deprecation policy

- Pre-1.0: When feasible, prefer a deprecation cycle rather than immediate removal. Mark items as `#[deprecated(note = "…")]` (or deprecate via documentation for macros) and keep them for at least one minor release before removal.
- Post-1.0: A typical deprecation window is two minor releases. Deprecated items should include a note with a migration hint.


## MSRV and support window

- Minimum Supported Rust Version (MSRV): 1.70 (as specified in Cargo.toml).
- We will avoid raising MSRV in patch releases. MSRV can only increase in a minor release, with a note in the changelog.
- Support window: We aim to keep the latest minor release and the previous minor release in good shape with bug fixes and critical patches.


## Feature flags and stability

- `unstable` feature: All crates expose a no-op feature named `unstable`. Any experimental API or behavior with a chance of change or removal must be placed behind this feature prior to 1.0. Such items are not covered by the stability guarantees.
- Additive opt-in features: Non-breaking opt-in capabilities (e.g., `query-ext`, `batch-ext`, `stream-ext`) are acceptable and will remain optional by default. Their presence should not break existing users when disabled.
- Backend features: Backend selection features (e.g., `libsql-backend`, `postgres-backend`, `mysql-async`) are stable entry points to enable backends. Breaking internal refactors behind those features should not change their external behavior without a semver bump.


## Public API audit checklist (per PR)

Use this checklist when changing public APIs:

1) storeit_core
- [ ] Adding/removing/changing trait methods in `Repository<T>` or `RowAdapter<T>`?
- [ ] Changing bounds on generics (e.g., requiring additional traits) that could break users?
- [ ] Modifying `ParamValue` or `RepoError` variants?
- [ ] Transactions API signatures/behaviors changed?

2) storeit_macros
- [ ] New attributes or changes to existing attributes for `#[derive(Entity)]` or `#[repository(...)]`? Are defaults/backward compatibility maintained?
- [ ] Generated items (e.g., `*RowAdapter`, inherent `find_by_*` methods) changed names or signatures?
- [ ] Clear compile-time error messages and migration hints included?

3) storeit facade
- [ ] Re-exports changed (removed items, renamed modules)?
- [ ] Extension traits added/modified – are they behind a feature and additive?
- [ ] Backend re-exports (module path/names) stable?

4) storeit_sql_builder
- [ ] Builder function names and signatures stable? Return strings kept compatible?
- [ ] Placeholder style controlled by features unchanged?

5) Backends
- [ ] `*Repository<T, A>` constructor signatures stable?
- [ ] `TransactionManager` behavior visible to users unchanged or clearly documented?
- [ ] Type bounds and error behaviors compatible?

6) Cross-cutting
- [ ] Breaking change? If yes, either gate behind `unstable` feature or bump major version (post-1.0).
- [ ] Deprecation path planned (with notes and migration guidance)?
- [ ] Tests and examples updated accordingly.


## How to use `unstable`

- Introduce new experimental APIs with `#[cfg(feature = "unstable")]` (and relevant doc cfgs) in the crate where they live.
- Document in the item’s rustdoc that it is experimental and may change or be removed without notice.
- Before promoting to stable (removing `unstable`), provide a short deprecation/migration window if applicable.


## Communication

- Changes to stability policies, MSRV, or deprecations must be recorded in the CHANGELOG (to be introduced in release automation) and reflected in README + this document.

---

Questions or proposals for the stability policy? Open an issue with context and a suggested approach.
