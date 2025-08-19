# Cross-platform notes (Linux, macOS, Windows)

Last updated: 2025-08-19

This project targets stable Rust on Linux, macOS, and Windows. Most crates in this workspace are portable and have no OS-specific code. The backends interact with database drivers which may have OS-dependent nuances. This document summarizes practical notes by OS.

## What CI covers

- A dedicated workflow runs on Ubuntu, macOS, and Windows to build and lint the core crates and SQL builder:
  - storeit_core
  - storeit_macros (proc-macro crate; build-only)
  - storeit_sql_builder (+ tests)
- Backend adapters and containerized integration tests remain Linux-focused in CI to keep the matrix fast and reliable.

## Linux (Ubuntu)

- Default CI environment. All workflows (unit, feature matrix, integration with containers, and benches) already run here.
- No special caveats.

## macOS

- Building core crates and the SQL builder works out of the box with the stock Rust toolchain.
- If you run database backends locally:
  - Postgres: use Homebrew to install `postgresql`. Ensure server is running and `POSTGRES_URL` points to it.
  - MySQL: Homebrew `mysql` or `mariadb`. Some features may require enabling local auth plugins.
  - LibSQL/SQLite family: file paths use Unix-style separators; no path-length caveats like Windows.

## Windows

- Core crates and the SQL builder compile and tests pass under stable Rust on Windows.
- Common pitfalls when using database backends locally:
  - Path length: Long paths can still bite older toolchains; enable long paths in Windows settings if needed.
  - OpenSSL / SSL: Some MySQL/Postgres client configurations may require additional SSL libraries. Prefer driver builds that bundle/avoid OpenSSL or follow driver docs to install required runtime libraries.
  - Line endings: Not generally an issue for Rust/Cargo, but be mindful when copy-pasting SQL scripts.
- PowerShell vs. cmd: Cargo works in both; GitHub Actions uses PowerShell by default on `windows-latest`.

## Backends (general notes)

- Containerized integration tests are currently set up for Linux runners. Running them on macOS/Windows requires Docker Desktop; our tests skip gracefully when Docker is unavailable.
- For local development on Windows/macOS without Docker, you can:
  - Use libsql/SQLite in-memory to exercise repository logic quickly.
  - Point to remote Postgres/MySQL instances if you don’t want local services.

## Troubleshooting checklist

- Update Rust toolchain (`rustup update`) and confirm MSRV 1.70+.
- Clear cargo caches on CI issues and retry.
- For backend issues on Windows/macOS, verify client libraries and environment variables for connection URLs.
- For Windows path/permission errors, run shells as Admin only when absolutely necessary and prefer short project paths.
