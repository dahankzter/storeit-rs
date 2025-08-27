#![forbid(unsafe_code)]
#![cfg_attr(
    not(feature = "postgres-backend"),
    doc = "Enable feature `postgres-backend` to use this adapter."
)]

#[cfg(feature = "postgres-backend")]
mod backend {
    use async_trait::async_trait;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::marker::PhantomData;
    use std::sync::Mutex;
    use storeit_core::transactions::{
        Isolation, Propagation, TransactionContext, TransactionDefinition, TransactionManager,
    };
    use storeit_core::{
        Fetchable, Identifiable, Insertable, ParamValue, RepoError, RepoResult, Repository,
        RowAdapter, Updatable,
    };
    use tokio_postgres::{
        types::{FromSql, ToSql},
        Client, NoTls, Row,
    };

    // Task-local state for current transaction client and savepoint depth.
    tokio::task_local! {
        static PG_TX_STACK: RefCell<Vec<std::sync::Arc<Client>>>;
        static PG_SP_DEPTH: RefCell<usize>;
    }

    fn isolation_sql(isolation: Isolation) -> Option<&'static str> {
        match isolation {
            Isolation::Default => None,
            Isolation::ReadCommitted => Some("SET TRANSACTION ISOLATION LEVEL READ COMMITTED"),
            Isolation::RepeatableRead => Some("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ"),
            Isolation::Serializable => Some("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE"),
        }
    }

    /// Concrete TransactionManager for Postgres.
    #[derive(Clone, Debug)]
    pub struct TokioPostgresTransactionManager {
        conn_str: String,
    }

    impl TokioPostgresTransactionManager {
        pub fn new<S: Into<String>>(conn_str: S) -> Self {
            Self {
                conn_str: conn_str.into(),
            }
        }

        async fn connect(&self) -> RepoResult<Client> {
            let (client, connection) = tokio_postgres::connect(&self.conn_str, NoTls)
                .await
                .map_err(RepoError::backend)?;
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("Postgres connection error: {}", e);
                }
            });
            Ok(client)
        }

        /// Vend a repository bound to the current transaction client if available, otherwise
        /// a repository backed by a fresh client connection.
        pub async fn repository<T, A>(
            &self,
            _ctx: TransactionContext<'_>,
            adapter: A,
        ) -> RepoResult<TokioPostgresRepository<T, A>>
        where
            T: Fetchable + Identifiable + Insertable + Updatable + Send + Sync + 'static,
            A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
            T::Key: Clone + Send + Sync + ToSql + Sync,
        {
            // If a transaction-bound client is present in task-local storage, use it.
            if let Ok(Some(arc_client)) = PG_TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                return Ok(TokioPostgresRepository {
                    client: arc_client,
                    adapter,
                    sql: RepoSql::<T>::new(),
                    _marker: PhantomData,
                });
            }
            // Otherwise, connect a fresh client.
            let client = self.connect().await?;
            Ok(TokioPostgresRepository::new(client, adapter))
        }
    }

    /// A helper to convert `ParamValue`s into a `Vec` of owned, boxed `ToSql` trait objects.
    /// This is necessary to manage the lifetimes of the parameters correctly.
    fn to_postgres_params(values: &[ParamValue]) -> Vec<Box<dyn ToSql + Sync + Send>> {
        values
            .iter()
            .map(|v| -> Box<dyn ToSql + Sync + Send> {
                match v {
                    ParamValue::String(s) => Box::new(s.clone()),
                    ParamValue::I32(i) => Box::new(*i),
                    ParamValue::I64(i) => Box::new(*i),
                    ParamValue::F64(f) => Box::new(*f),
                    ParamValue::Bool(b) => Box::new(*b),
                    ParamValue::Null => Box::new(Option::<i32>::None),
                }
            })
            .collect()
    }

    /// Prebuilt SQL strings for common operations, computed once per repository instance.
    struct RepoSql<T> {
        select_by_id: String,
        delete_by_id: String,
        insert: String,
        update_by_id: String,
        find_by_field_cache: Mutex<HashMap<String, String>>,
        _marker: PhantomData<T>,
    }

    impl<T> RepoSql<T>
    where
        T: Fetchable + Identifiable + Insertable + Updatable,
    {
        fn new() -> Self {
            let select_by_id = storeit_sql_builder::select_by_id::<T>(T::ID_COLUMN);
            let delete_by_id = storeit_sql_builder::delete_by_id::<T>(T::ID_COLUMN);
            let insert = storeit_sql_builder::insert::<T>(T::ID_COLUMN);
            let update_by_id = storeit_sql_builder::update_by_id::<T>(T::ID_COLUMN);
            Self {
                select_by_id,
                delete_by_id,
                insert,
                update_by_id,
                find_by_field_cache: Mutex::new(HashMap::new()),
                _marker: PhantomData,
            }
        }

        fn get_select_by_field(&self, field: &str) -> String
        where
            T: Fetchable,
        {
            let mut guard = self.find_by_field_cache.lock().unwrap();
            if let Some(s) = guard.get(field) {
                return s.clone();
            }
            let built = storeit_sql_builder::select_by_field::<T>(field);
            guard.insert(field.to_string(), built.clone());
            built
        }
    }

    /// A fully asynchronous, `tokio-postgres`-backed repository.
    pub struct TokioPostgresRepository<T, A>
    where
        T: Identifiable + 'static,
        A: RowAdapter<T> + Send + Sync + 'static,
    {
        client: std::sync::Arc<Client>,
        adapter: A,
        sql: RepoSql<T>,
        _marker: PhantomData<T>,
    }

    impl<T, A> TokioPostgresRepository<T, A>
    where
        T: Fetchable + Identifiable + Send + Sync + 'static,
        A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
        T::Key: Clone + Send + Sync + ToSql + Sync,
    {
        /// Creates a new repository from an existing `tokio_postgres::Client`.
        pub fn new(client: Client, adapter: A) -> Self
        where
            T: Insertable + Updatable,
        {
            let sql = RepoSql::<T>::new();
            Self {
                client: std::sync::Arc::new(client),
                adapter,
                sql,
                _marker: PhantomData,
            }
        }

        /// Creates a new repository by connecting to a database URL.
        pub async fn from_url(
            conn_str: &str,
            _id_column: &str, // Note: id_column is now read from T::ID_COLUMN
            adapter: A,
        ) -> RepoResult<Self>
        where
            T: Insertable + Updatable,
        {
            let (client, connection) = tokio_postgres::connect(conn_str, NoTls)
                .await
                .map_err(RepoError::backend)?;
            // The connection object must be spawned to process network events.
            tokio::spawn(async move {
                if let Err(e) = connection.await {
                    eprintln!("Postgres connection error: {}", e);
                }
            });
            Ok(Self::new(client, adapter))
        }
    }

    #[async_trait]
    impl TransactionManager for TokioPostgresTransactionManager {
        async fn execute<'a, R, F, Fut>(
            &'a self,
            def: &TransactionDefinition,
            f: F,
        ) -> RepoResult<R>
        where
            F: FnOnce(TransactionContext<'a>) -> Fut + Send + 'a,
            Fut: core::future::Future<Output = RepoResult<R>> + Send + 'a,
            R: Send + 'a,
        {
            // Core logic in async block, executed within task-local scopes if necessary
            let fut = async {
                let mut created_tx = false;
                let mut used_savepoint = false;
                let active = PG_TX_STACK
                    .try_with(|cell| !cell.borrow().is_empty())
                    .unwrap_or(false);

                if matches!(
                    def.propagation,
                    Propagation::NotSupported | Propagation::Supports
                ) && !active
                {
                    return f(TransactionContext::new()).await;
                }
                if matches!(def.propagation, Propagation::Never) && active {
                    return Err(RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Transaction exists but Propagation::Never requested",
                    )));
                }

                // Acquire client
                let client_arc: std::sync::Arc<Client> = if active {
                    PG_TX_STACK.with(|cell| cell.borrow().last().cloned().expect("stack non-empty"))
                } else {
                    let client = self.connect().await?;
                    std::sync::Arc::new(client)
                };

                if !active {
                    client_arc
                        .batch_execute("BEGIN")
                        .await
                        .map_err(RepoError::backend)?;
                    if let Some(sql) = isolation_sql(def.isolation) {
                        client_arc.batch_execute(sql).await.ok();
                    }
                    if def.read_only {
                        client_arc
                            .batch_execute("SET TRANSACTION READ ONLY")
                            .await
                            .ok();
                    }
                    if let Some(to) = def.timeout {
                        client_arc
                            .batch_execute(&format!(
                                "SET LOCAL statement_timeout = '{}ms'",
                                to.as_millis()
                            ))
                            .await
                            .ok();
                    }
                    PG_TX_STACK.with(|cell| cell.borrow_mut().push(client_arc.clone()));
                    PG_SP_DEPTH.with(|d| *d.borrow_mut() = 0);
                    created_tx = true;
                } else {
                    match def.propagation {
                        Propagation::RequiresNew | Propagation::Nested => {
                            let depth = PG_SP_DEPTH.with(|d| *d.borrow());
                            let name = format!("sp{}", depth + 1);
                            client_arc
                                .batch_execute(&format!("SAVEPOINT {}", name))
                                .await
                                .ok();
                            PG_SP_DEPTH.with(|d| *d.borrow_mut() += 1);
                            used_savepoint = true;
                        }
                        _ => {}
                    }
                }

                let result = f(TransactionContext::new()).await;

                if created_tx {
                    if result.is_ok() {
                        client_arc
                            .batch_execute("COMMIT")
                            .await
                            .map_err(RepoError::backend)?;
                    } else {
                        client_arc
                            .batch_execute("ROLLBACK")
                            .await
                            .map_err(RepoError::backend)?;
                    }
                    PG_TX_STACK.with(|cell| {
                        let _ = cell.borrow_mut().pop();
                    });
                } else if used_savepoint {
                    let name = PG_SP_DEPTH.with(|d| {
                        let v = *d.borrow();
                        format!("sp{}", v)
                    });
                    if result.is_ok() {
                        client_arc
                            .batch_execute(&format!("RELEASE SAVEPOINT {}", name))
                            .await
                            .ok();
                    } else {
                        client_arc
                            .batch_execute(&format!("ROLLBACK TO SAVEPOINT {}", name))
                            .await
                            .ok();
                    }
                    PG_SP_DEPTH.with(|d| {
                        let mut b = d.borrow_mut();
                        if *b > 0 {
                            *b -= 1;
                        }
                    });
                }

                result
            };

            // Initialize task-local scopes if needed
            let not_initialized = PG_TX_STACK.try_with(|_| ()).is_err();
            if not_initialized {
                PG_TX_STACK
                    .scope(RefCell::new(Vec::new()), async move {
                        PG_SP_DEPTH.scope(RefCell::new(0usize), fut).await
                    })
                    .await
            } else {
                fut.await
            }
        }
    }

    #[async_trait]
    impl<T, A> Repository<T> for TokioPostgresRepository<T, A>
    where
        T: Fetchable + Identifiable + Insertable + Updatable + Send + Sync + Clone + 'static,
        A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
        T::Key: Clone
            + Send
            + Sync
            + 'static
            + ToSql
            + Sync
            + Default
            + PartialEq
            + for<'b> FromSql<'b>,
    {
        async fn find_by_id(&self, id: &T::Key) -> RepoResult<Option<T>> {
            // Prefer an active transaction-bound client if present in task-local storage.
            let client = if let Ok(Some(arc_client)) =
                PG_TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                arc_client
            } else {
                self.client.clone()
            };
            let row_opt = client
                .query_opt(&self.sql.select_by_id, &[id])
                .await
                .map_err(RepoError::backend)?;

            match row_opt {
                Some(row) => Ok(Some(self.adapter.from_row(&row)?)),
                None => Ok(None),
            }
        }

        async fn find_by_field(&self, field_name: &str, value: ParamValue) -> RepoResult<Vec<T>> {
            let sql = self.sql.get_select_by_field(field_name);

            // Prefer an active transaction-bound client if present in task-local storage.
            let client = if let Ok(Some(arc_client)) =
                PG_TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                arc_client
            } else {
                self.client.clone()
            };

            // Match on the ParamValue to create a temporary value of the correct type,
            // which can then be passed as a `ToSql` trait object.
            let rows = match value {
                ParamValue::String(s) => client
                    .query(&sql, &[&s])
                    .await
                    .map_err(RepoError::backend)?,
                ParamValue::I32(i) => client
                    .query(&sql, &[&i])
                    .await
                    .map_err(RepoError::backend)?,
                ParamValue::I64(i) => client
                    .query(&sql, &[&i])
                    .await
                    .map_err(RepoError::backend)?,
                ParamValue::F64(f) => client
                    .query(&sql, &[&f])
                    .await
                    .map_err(RepoError::backend)?,
                ParamValue::Bool(b) => client
                    .query(&sql, &[&b])
                    .await
                    .map_err(RepoError::backend)?,
                ParamValue::Null => client
                    .query(&sql, &[&Option::<i32>::None])
                    .await
                    .map_err(RepoError::backend)?,
            };

            rows.iter()
                .map(|row| self.adapter.from_row(row))
                .collect::<RepoResult<Vec<T>>>()
        }

        async fn insert(&self, entity: &T) -> RepoResult<T> {
            let param_values = entity.insert_values();
            let owned_params = to_postgres_params(&param_values);
            let params: Vec<&(dyn ToSql + Sync)> = owned_params
                .iter()
                .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                .collect();

            // Prefer an active transaction-bound client if present in task-local storage.
            let client = if let Ok(Some(arc_client)) =
                PG_TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                arc_client
            } else {
                self.client.clone()
            };

            let row = client
                .query_one(&self.sql.insert, &params[..])
                .await
                .map_err(RepoError::backend)?;
            let new_id: T::Key = row.get(0);

            self.find_by_id(&new_id).await.and_then(|opt| {
                opt.ok_or_else(|| {
                    RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Failed to fetch entity after insert",
                    ))
                })
            })
        }

        async fn update(&self, entity: &T) -> RepoResult<T> {
            let param_values = entity.update_values();
            let owned_params = to_postgres_params(&param_values);
            let params: Vec<&(dyn ToSql + Sync)> = owned_params
                .iter()
                .map(|p| p.as_ref() as &(dyn ToSql + Sync))
                .collect();

            // Prefer an active transaction-bound client if present in task-local storage.
            let client = if let Ok(Some(arc_client)) =
                PG_TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                arc_client
            } else {
                self.client.clone()
            };

            client
                .execute(&self.sql.update_by_id, &params[..])
                .await
                .map_err(RepoError::backend)?;

            Ok(entity.clone())
        }

        async fn delete_by_id(&self, id: &T::Key) -> RepoResult<bool> {
            // Prefer an active transaction-bound client if present in task-local storage.
            let client = if let Ok(Some(arc_client)) =
                PG_TX_STACK.try_with(|cell| cell.borrow().last().cloned())
            {
                arc_client
            } else {
                self.client.clone()
            };
            let n = client
                .execute(&self.sql.delete_by_id, &[id])
                .await
                .map_err(RepoError::backend)?;
            Ok(n > 0)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::backend::{isolation_sql, to_postgres_params, RepoSql};
        use storeit_core::{Fetchable, Identifiable, Insertable, ParamValue, Updatable};

        // A tiny dummy entity to exercise RepoSql without any database.
        #[derive(Clone)]
        struct Dummy;
        impl Fetchable for Dummy {
            const TABLE: &'static str = "dummy";
            const SELECT_COLUMNS: &'static [&'static str] = &["id", "name"];
            const FINDABLE_COLUMNS: &'static [(&'static str, &'static str)] = &[("name", "TEXT")];
        }
        impl Identifiable for Dummy {
            type Key = i64;
            const ID_COLUMN: &'static str = "id";
            fn id(&self) -> Option<Self::Key> {
                None
            }
        }
        impl Insertable for Dummy {
            const INSERT_COLUMNS: &'static [&'static str] = &["name"];
            fn insert_values(&self) -> Vec<ParamValue> {
                vec![ParamValue::String("x".into())]
            }
        }
        impl Updatable for Dummy {
            const UPDATE_COLUMNS: &'static [&'static str] = &["name", "id"];
            fn update_values(&self) -> Vec<ParamValue> {
                vec![ParamValue::String("y".into()), ParamValue::I64(1)]
            }
        }

        // Smoke-test the ParamValue -> ToSql boxing helper. We can't downcast trait objects here,
        // but we can at least assert length and that no panic occurs for all variants.
        #[test]
        fn to_postgres_params_maps_all_variants() {
            let values = [
                ParamValue::String("s".to_string()),
                ParamValue::I32(1),
                ParamValue::I64(2),
                ParamValue::F64(3.5),
                ParamValue::Bool(true),
                ParamValue::Null,
            ];
            let boxed = to_postgres_params(&values);
            assert_eq!(boxed.len(), values.len());
        }

        #[test]
        fn isolation_sql_maps_variants() {
            // Default has no statement; others do.
            assert!(isolation_sql(Isolation::Default).is_none());
            assert!(isolation_sql(Isolation::ReadCommitted)
                .unwrap()
                .contains("READ COMMITTED"));
            assert!(isolation_sql(Isolation::RepeatableRead)
                .unwrap()
                .contains("REPEATABLE READ"));
            assert!(isolation_sql(Isolation::Serializable)
                .unwrap()
                .contains("SERIALIZABLE"));
        }

        #[test]
        fn repo_sql_builds_expected_statements_and_caches() {
            let sql = RepoSql::<Dummy>::new();
            // Basic statements should mention table name "dummy".
            assert!(sql.select_by_id.to_lowercase().contains("dummy"));
            assert!(sql.delete_by_id.to_lowercase().contains("dummy"));
            assert!(sql.insert.to_lowercase().contains("dummy"));
            assert!(sql.update_by_id.to_lowercase().contains("dummy"));

            // get_select_by_field should be deterministic and cached; repeated calls equal.
            let f1 = sql.get_select_by_field("name");
            let f2 = sql.get_select_by_field("name");
            assert_eq!(f1, f2);
            assert!(f1.to_lowercase().contains("where"));

            // Different field yields a different SQL string (most likely); at least it builds.
            let f_other = sql.get_select_by_field("id");
            assert!(!f_other.is_empty());
        }
    }
}

#[cfg(feature = "postgres-backend")]
pub use backend::{TokioPostgresRepository, TokioPostgresTransactionManager};
