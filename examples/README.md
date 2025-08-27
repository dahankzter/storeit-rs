# End-to-end examples

This repository includes minimal end-to-end usage examples for each backend. Examples are placed under the `repository` facade crate so you can run them with `cargo run -p repository ...`.

Schemas (migrations) are provided as inline SQL in the examples and also as standalone files under `examples/migrations`.

## Prerequisites
- Rust stable toolchain
- For Postgres/MySQL examples: a running database and a connection URL in environment variables (see below).

## LibSQL (SQLite-family)
Run:

```
cargo run -p repository --no-default-features --features libsql-backend --example libsql_e2e
```

Transactions-focused example (commit + rollback):

```
cargo run -p repository --no-default-features --features libsql-backend --example libsql_tx
```

This uses an in-memory database and creates the `users` table on the fly. No additional setup is required.

## Postgres (tokio_postgres)
Export a connection URL (or rely on the default in the example):

```
export POSTGRES_URL=postgres://postgres:postgres@localhost:5432/postgres
cargo run -p repository --no-default-features --features postgres-backend --example postgres_e2e
```

The example will create the `users` table if it does not exist.

## MySQL (mysql_async)
Export a connection URL (or rely on the default in the example):

```
export MYSQL_URL=mysql://root:root@localhost:3306/test
cargo run -p repository --no-default-features --features mysql-async --example mysql_e2e
```

The example will create the `users` table if it does not exist.

## What the examples demonstrate
- Defining an entity with `#[derive(Entity)]`
- Generating a typed repository with `#[repository(...)]` including a finder (e.g., `find_by_email`)
- Providing a simple manual `RowAdapter` per backend
- Basic CRUD: insert, find by id, finder, update, delete
- Transactions:
  - Backend-agnostic shape via `TransactionTemplate`
  - Real transactional semantics with `storeit_libsql::LibsqlTransactionManager` in libsql examples

## Web framework examples (libsql in-memory)
- Axum: `cargo run -p repository --no-default-features --features libsql-backend --example axum_libsql`
- Warp: `cargo run -p repository --no-default-features --features libsql-backend --example warp_libsql`
- Actix-Web: `cargo run -p repository --no-default-features --features libsql-backend --example actix_libsql`

Each server exposes:
- GET /health → "ok"
- GET /users/:id → fetch by id
- GET /users?email=foo@bar → finder
- POST /users {email, active} → create inside a transaction

## Standalone migration files
See `examples/migrations` for minimal `users` table DDL per backend. Apply them with your usual migration tool if you prefer not to let the examples create the schema automatically.
