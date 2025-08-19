# Makefile for storeit-rs workspace
# Common developer tasks and coverage with auto-open

# Detect OS for opening files
UNAME_S := $(shell uname -s)
OPEN_CMD := xdg-open
ifeq ($(UNAME_S),Darwin)
  OPEN_CMD := open
endif
# Basic Windows detection (when using GNU Make on Windows)
ifeq ($(OS),Windows_NT)
  OPEN_CMD := start
endif

CARGO := cargo
COV_HTML_INDEX := target/llvm-cov/html/index.html
# Default to skipping container-based tests in integration-backends unless overridden by the caller
# Set RUN_CONTAINERS=1 or SKIP_CONTAINER_TESTS=0 when you explicitly want to run containers.
SKIP_CONTAINER_TESTS ?= 1
ifdef RUN_CONTAINERS
SKIP_CONTAINER_TESTS := 0
endif

.PHONY: help tools fmt clippy build test doc clean coverage coverage-html coverage-lcov coverage-enforce coverage-summary coverage-merge integration-backends

help:
	@echo "Available targets:"
	@echo "  help               - Show this help"
	@echo "  tools              - Install coverage prerequisites (cargo-llvm-cov, llvm-tools-preview)"
	@echo "  fmt                - Run rustfmt on all crates"
	@echo "  clippy             - Run clippy on all targets with all features (deny warnings)"
	@echo "  build              - Build the entire workspace with all features"
	@echo "  test               - Run tests for the entire workspace with all features"
	@echo "  doc                - Build docs for the workspace (no deps)"
	@echo "  clean              - Clean target artifacts"
	@echo "  coverage           - Generate HTML coverage report and open it"
	@echo "  coverage-html      - Generate HTML coverage report (no open)"
	@echo "  coverage-lcov      - Generate lcov.info coverage file in repo root"
	@echo "  coverage-enforce   - Fail if coverage is below 100%"
	@echo "  coverage-summary   - Print concise coverage summary for the workspace"
	@echo "  coverage-merge     - Merge coverage across default and tokio_postgres features (HTML)"
	@echo "  coverage-all       - Full merged coverage including integration tests (skip containers by default)"
	@echo "  coverage-all-summary - Same as coverage-all but prints a summary only"
	@echo "  integration-backends - Run ignored integration tests for all backends (containers required)"

# Tooling install: cargo-llvm-cov and llvm-tools-preview component
# Safe to re-run; will be no-ops if already installed
tools:
	rustup component add llvm-tools-preview
	rustup component add rustfmt
	rustup component add clippy
	@# Install cargo-llvm-cov if missing (or update to locked version)
	@if ! command -v cargo-llvm-cov >/dev/null 2>&1; then \
		$(CARGO) install cargo-llvm-cov --locked; \
	else \
		echo "cargo-llvm-cov already installed"; \
	fi

fmt:
	$(CARGO) fmt --all

clippy:
	$(CARGO) clippy --workspace --all-targets --all-features -D warnings

build:
	$(CARGO) build --workspace --all-features

test:
	$(CARGO) test --workspace --all-features

doc:
	$(CARGO) doc --workspace --no-deps --all-features

clean:
	$(CARGO) clean

# Coverage using workspace Cargo aliases defined in Cargo.toml
coverage-html: tools
	$(CARGO) coverage-html

coverage-lcov: tools
	$(CARGO) coverage-lcov

coverage: coverage-html
	@if [ -f "$(COV_HTML_INDEX)" ]; then \
		$(OPEN_CMD) "$(COV_HTML_INDEX)" >/dev/null 2>&1 || true; \
		echo "Opened coverage report: $(COV_HTML_INDEX)"; \
	else \
		echo "Coverage HTML not found at $(COV_HTML_INDEX)"; \
		echo "Run: make coverage-html"; \
	fi

coverage-enforce: tools
	$(CARGO) llvm-cov --workspace --all-features --fail-under-lines 100

coverage-summary: tools
	$(CARGO) llvm-cov --workspace --all-features --summary-only

# Merge coverage across default features and tokio_postgres to cover cfg-gated lines
coverage-merge: tools
	$(CARGO) llvm-cov clean --workspace
	$(CARGO) llvm-cov --workspace --no-report
	$(CARGO) llvm-cov --workspace --features tokio_postgres --no-report
	$(CARGO) llvm-cov --workspace --html
	@if [ -f "$(COV_HTML_INDEX)" ]; then \
		$(OPEN_CMD) "$(COV_HTML_INDEX)" >/dev/null 2>&1 || true; \
		echo "Merged coverage report generated at $(COV_HTML_INDEX)"; \
	else \
		echo "Coverage HTML not found at $(COV_HTML_INDEX)"; \
	fi

# Run all ignored backend integration tests (requires Docker)
# By default, container-based tests are skipped to avoid requiring Docker.
# To run containers, invoke as: RUN_CONTAINERS=1 make integration-backends
integration-backends:
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) test -p storeit_libsql --features libsql-backend -- --ignored
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) test -p storeit_mysql_async --features mysql-async -- --ignored
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) test -p storeit_tokio_postgres --features postgres-backend -- --ignored

# Full coverage including (optionally) container-based integration tests.
# - Default: skip container tests and generate merged HTML coverage for the workspace.
# - With containers: RUN_CONTAINERS=1 make coverage-all (sets SKIP_CONTAINER_TESTS=0)
coverage-all: tools
	$(CARGO) llvm-cov clean --workspace
	# 1) Workspace (all features), no report yet
	$(CARGO) llvm-cov --workspace --all-features --no-report
	# 2) Merge backend integrations (ignored tests) per package
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) llvm-cov --package storeit_libsql --features libsql-backend --no-report -- --ignored
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) llvm-cov --package storeit_mysql_async --features mysql-async --no-report -- --ignored
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) llvm-cov --package storeit_tokio_postgres --features postgres-backend --no-report -- --ignored
	# 3) Emit final HTML report and open it
	$(CARGO) llvm-cov report --html
	@if [ -f "$(COV_HTML_INDEX)" ]; then \
		$(OPEN_CMD) "$(COV_HTML_INDEX)" >/dev/null 2>&1 || true; \
		echo "Full coverage report generated at $(COV_HTML_INDEX)"; \
	else \
		echo "Coverage HTML not found at $(COV_HTML_INDEX)"; \
	fi

# Same as coverage-all but prints a concise summary without HTML
coverage-all-summary: tools
	$(CARGO) llvm-cov clean --workspace
	$(CARGO) llvm-cov --workspace --all-features --no-report
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) llvm-cov --package storeit_libsql --features libsql-backend --no-report -- --ignored
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) llvm-cov --package storeit_mysql_async --features mysql-async --no-report -- --ignored
	SKIP_CONTAINER_TESTS=$(SKIP_CONTAINER_TESTS) $(CARGO) llvm-cov --package storeit_tokio_postgres --features postgres-backend --no-report -- --ignored
	$(CARGO) llvm-cov report --summary-only
