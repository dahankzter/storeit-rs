#![forbid(unsafe_code)]
#![cfg_attr(
    not(feature = "mysql-async"),
    doc = "This crate provides a mysql_async backend adapter. Enable feature `mysql-async` to use it."
)]

#[cfg(feature = "mysql-async")]
mod backend {
    use async_trait::async_trait;
    use mysql_async::{prelude::*, Conn, Params, Pool, Row, Value};
    use std::collections::HashMap;
    use std::marker::PhantomData;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use storeit_core::transactions::{
        Isolation, Propagation, TransactionContext, TransactionDefinition, TransactionManager,
    };
    use storeit_core::{
        Fetchable, Identifiable, Insertable, ParamValue, RepoError, RepoResult, Repository,
        RowAdapter, Updatable,
    };
    use tokio::sync::Mutex;

    // Task-local storage for a transaction-bound connection and savepoint depth.
    tokio::task_local! {
        static MY_TX_CONN: std::cell::RefCell<Option<Arc<Mutex<Conn>>>>;
        static MY_SP_DEPTH: std::cell::RefCell<usize>;
    }

    // Helper to convert ParamValue to mysql_async::Value.
    fn to_mysql_value(p: ParamValue) -> Value {
        match p {
            ParamValue::String(s) => Value::from(s),
            ParamValue::I32(i) => Value::from(i),
            ParamValue::I64(i) => Value::from(i),
            ParamValue::F64(f) => Value::from(f),
            ParamValue::Bool(b) => Value::from(b),
            ParamValue::Null => Value::NULL,
        }
    }

    /// A fully asynchronous, `mysql_async`-backed repository.
    struct RepoSql<T> {
        select_by_id: String,
        delete_by_id: String,
        insert: String,
        update_by_id: String,
        find_by_field_cache: StdMutex<HashMap<String, String>>,
        _phantom: PhantomData<T>,
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
                find_by_field_cache: StdMutex::new(HashMap::new()),
                _phantom: PhantomData,
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

    pub struct MysqlAsyncRepository<T, A>
    where
        T: Identifiable + 'static,
        A: RowAdapter<T> + Send + Sync + 'static,
    {
        pool: Pool,
        adapter: A,
        sql: RepoSql<T>,
        _phantom: PhantomData<T>,
    }

    impl<T, A> MysqlAsyncRepository<T, A>
    where
        A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
        T: Fetchable + Identifiable + Send + Sync + 'static,
        T::Key: Clone + Into<Value> + Send + Sync,
    {
        /// Creates a new repository from a mysql_async Pool.
        pub fn new(pool: Pool, adapter: A) -> Self
        where
            T: Insertable + Updatable,
        {
            let sql = RepoSql::<T>::new();
            Self {
                pool,
                adapter,
                sql,
                _phantom: PhantomData,
            }
        }

        /// Creates a new repository by connecting to a database URL.
        pub async fn from_url(
            database_url: &str,
            _id_column: &str, // Note: id_column is now read from T::ID_COLUMN
            adapter: A,
        ) -> RepoResult<Self>
        where
            T: Insertable + Updatable,
        {
            let pool = Pool::new(database_url);
            Ok(Self::new(pool, adapter))
        }

        async fn get_conn(&self) -> RepoResult<Conn> {
            self.pool.get_conn().await.map_err(RepoError::backend)
        }

        // Note on transactions and repository reuse:
        // This repository consults a task-local transaction connection (MY_TX_CONN)
        // on every operation. If a transaction is active (managed by
        // MysqlAsyncTransactionManager::execute), methods prefer that connection;
        // otherwise, a fresh connection is acquired from the pool. This design lets
        // applications create a repository once and reuse the same instance both
        // outside and inside transactions — calls within a transaction automatically
        // participate in it without recreating the repository.
        #[allow(dead_code)]
        async fn with_conn<F, Fut, R>(&self, f: F) -> RepoResult<R>
        where
            F: FnOnce(&mut Conn) -> Fut,
            Fut: core::future::Future<Output = RepoResult<R>>,
        {
            // If we have a transaction-bound connection in task-local storage, use it.
            if let Ok(Some(arc)) = MY_TX_CONN.try_with(|c| c.borrow().as_ref().cloned()) {
                let mut guard = arc.lock().await;
                return f(&mut guard).await;
            }
            let mut conn = self.get_conn().await?;
            let res = f(&mut conn).await;
            drop(conn);
            res
        }
    }

    #[async_trait]
    impl<T, A> Repository<T> for MysqlAsyncRepository<T, A>
    where
        A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
        T: Fetchable + Identifiable + Insertable + Updatable + Send + Sync + Clone + 'static,
        T::Key: Clone
            + Into<Value>
            + Send
            + Sync
            + 'static
            + Default
            + PartialEq
            + serde::Serialize
            + serde::de::DeserializeOwned,
    {
        async fn find_by_id(&self, id: &T::Key) -> RepoResult<Option<T>> {
            let id_val: Value = id.clone().into();
            let row_opt: Option<Row> =
                if let Ok(Some(arc)) = MY_TX_CONN.try_with(|c| c.borrow().as_ref().cloned()) {
                    let mut conn = arc.lock().await;
                    conn.exec_first(
                        self.sql.select_by_id.clone(),
                        Params::Positional(vec![id_val]),
                    )
                    .await
                    .map_err(RepoError::backend)?
                } else {
                    let mut conn = self.get_conn().await?;
                    conn.exec_first(
                        self.sql.select_by_id.clone(),
                        Params::Positional(vec![id_val]),
                    )
                    .await
                    .map_err(RepoError::backend)?
                };

            let entity_opt = match row_opt {
                Some(ref row) => Some(self.adapter.from_row(row)?),
                None => None,
            };
            Ok(entity_opt)
        }

        async fn find_by_field(&self, field_name: &str, value: ParamValue) -> RepoResult<Vec<T>> {
            let sql = self.sql.get_select_by_field(field_name);
            let value_param = to_mysql_value(value);
            let rows: Vec<Row> =
                if let Ok(Some(arc)) = MY_TX_CONN.try_with(|c| c.borrow().as_ref().cloned()) {
                    let mut conn = arc.lock().await;
                    conn.exec(sql, Params::Positional(vec![value_param]))
                        .await
                        .map_err(RepoError::backend)?
                } else {
                    let mut conn = self.get_conn().await?;
                    conn.exec(sql, Params::Positional(vec![value_param]))
                        .await
                        .map_err(RepoError::backend)?
                };

            rows.iter()
                .map(|row| self.adapter.from_row(row))
                .collect::<RepoResult<Vec<T>>>()
        }

        async fn insert(&self, entity: &T) -> RepoResult<T> {
            let params = Params::Positional(
                entity
                    .insert_values()
                    .into_iter()
                    .map(to_mysql_value)
                    .collect(),
            );
            let (new_id, ()) =
                if let Ok(Some(arc)) = MY_TX_CONN.try_with(|c| c.borrow().as_ref().cloned()) {
                    let mut conn = arc.lock().await;
                    let mut result = conn
                        .exec_iter(self.sql.insert.clone(), params)
                        .await
                        .map_err(RepoError::backend)?;
                    let new_id = result.last_insert_id().unwrap_or(0);
                    result.map(|_| ()).await.map_err(RepoError::backend)?;
                    (new_id, ())
                } else {
                    let mut conn = self.get_conn().await?;
                    let mut result = conn
                        .exec_iter(self.sql.insert.clone(), params)
                        .await
                        .map_err(RepoError::backend)?;
                    let new_id = result.last_insert_id().unwrap_or(0);
                    result.map(|_| ()).await.map_err(RepoError::backend)?;
                    (new_id, ())
                };

            let key_val: T::Key = serde_json::from_value(serde_json::Value::from(new_id))
                .map_err(RepoError::backend)?;

            self.find_by_id(&key_val).await.and_then(|opt| {
                opt.ok_or_else(|| {
                    RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "failed to fetch entity after insert",
                    ))
                })
            })
        }

        async fn update(&self, entity: &T) -> RepoResult<T> {
            let params = Params::Positional(
                entity
                    .update_values()
                    .into_iter()
                    .map(to_mysql_value)
                    .collect(),
            );
            if let Ok(Some(arc)) = MY_TX_CONN.try_with(|c| c.borrow().as_ref().cloned()) {
                let mut conn = arc.lock().await;
                conn.exec_drop(self.sql.update_by_id.clone(), params)
                    .await
                    .map_err(RepoError::backend)?;
            } else {
                let mut conn = self.get_conn().await?;
                conn.exec_drop(self.sql.update_by_id.clone(), params)
                    .await
                    .map_err(RepoError::backend)?;
            }

            Ok(entity.clone())
        }

        async fn delete_by_id(&self, id: &T::Key) -> RepoResult<bool> {
            let id_val: Value = id.clone().into();
            let affected =
                if let Ok(Some(arc)) = MY_TX_CONN.try_with(|c| c.borrow().as_ref().cloned()) {
                    let mut conn = arc.lock().await;
                    let result = conn
                        .exec_iter(
                            self.sql.delete_by_id.clone(),
                            Params::Positional(vec![id_val]),
                        )
                        .await
                        .map_err(RepoError::backend)?;
                    result.affected_rows()
                } else {
                    let mut conn = self.get_conn().await?;
                    let result = conn
                        .exec_iter(
                            self.sql.delete_by_id.clone(),
                            Params::Positional(vec![id_val]),
                        )
                        .await
                        .map_err(RepoError::backend)?;
                    result.affected_rows()
                };
            Ok(affected > 0)
        }
    }

    /// A concrete TransactionManager for mysql_async using a single connection per transaction.
    #[derive(Clone, Debug)]
    pub struct MysqlAsyncTransactionManager {
        pool: Pool,
    }

    impl MysqlAsyncTransactionManager {
        pub fn new(pool: Pool) -> Self {
            Self { pool }
        }

        pub async fn repository<T, A>(
            &self,
            _ctx: TransactionContext<'_>,
            adapter: A,
        ) -> RepoResult<MysqlAsyncRepository<T, A>>
        where
            T: Fetchable + Identifiable + Insertable + Updatable + Send + Sync + 'static,
            A: RowAdapter<T, Row = Row> + Send + Sync + 'static,
            T::Key: Clone + Into<Value> + Send + Sync,
        {
            // If there is a transaction-bound connection in TLS, use it; otherwise create a non-tx repo.
            // We don't need to capture the connection explicitly: the repository will pick up
            // the task-local transaction connection if present.
            let repo = MysqlAsyncRepository::new(self.pool.clone(), adapter);
            Ok(repo)
        }
    }

    #[async_trait]
    impl TransactionManager for MysqlAsyncTransactionManager {
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
            // Wrap in TLS scopes if needed
            let not_initialized = MY_TX_CONN.try_with(|_| ()).is_err();
            let run = async {
                let mut created_tx = false;
                let mut used_savepoint = false;
                let in_tx = MY_TX_CONN.with(|c| c.borrow().is_some());

                if matches!(
                    def.propagation,
                    Propagation::NotSupported | Propagation::Supports
                ) && !in_tx
                {
                    return f(TransactionContext::new()).await;
                }
                if matches!(def.propagation, Propagation::Never) && in_tx {
                    return Err(RepoError::backend(std::io::Error::new(
                        std::io::ErrorKind::Other,
                        "Transaction exists but Propagation::Never requested",
                    )));
                }

                if !in_tx {
                    let mut conn = self.pool.get_conn().await.map_err(RepoError::backend)?;
                    // Start transaction; attempt to set isolation/read-only/timeout best-effort
                    conn.query_drop("START TRANSACTION").await.ok();
                    match def.isolation {
                        Isolation::Default => {}
                        Isolation::ReadCommitted => {
                            conn.query_drop("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
                                .await
                                .ok();
                        }
                        Isolation::RepeatableRead => {
                            conn.query_drop("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                                .await
                                .ok();
                        }
                        Isolation::Serializable => {
                            conn.query_drop("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE")
                                .await
                                .ok();
                        }
                    }
                    if def.read_only {
                        conn.query_drop("SET TRANSACTION READ ONLY").await.ok();
                    }
                    if let Some(to) = def.timeout {
                        conn.query_drop(format!(
                            "SET SESSION innodb_lock_wait_timeout = {}",
                            to.as_secs()
                        ))
                        .await
                        .ok();
                    }

                    // Place connection into TLS (store as Arc<Mutex<Conn>>)
                    let arc = Arc::new(Mutex::new(conn));
                    MY_TX_CONN.with(|cell| {
                        *cell.borrow_mut() = Some(arc);
                    });
                    MY_SP_DEPTH.with(|d| *d.borrow_mut() = 0);
                    created_tx = true;
                } else {
                    // Savepoint for nested/RequiresNew
                    if matches!(
                        def.propagation,
                        Propagation::RequiresNew | Propagation::Nested
                    ) {
                        if let Some(arc) = MY_TX_CONN.with(|c| c.borrow().as_ref().cloned()) {
                            let mut conn = arc.lock().await;
                            let depth = MY_SP_DEPTH.with(|d| *d.borrow());
                            let name = format!("sp{}", depth + 1);
                            conn.query_drop(format!("SAVEPOINT {}", name)).await.ok();
                            MY_SP_DEPTH.with(|d| *d.borrow_mut() += 1);
                            used_savepoint = true;
                        }
                    }
                }

                let result = f(TransactionContext::new()).await;

                if created_tx {
                    if let Some(arc) = MY_TX_CONN.with(|c| c.borrow_mut().take()) {
                        let mut conn = arc.lock().await;
                        if result.is_ok() {
                            conn.query_drop("COMMIT").await.ok();
                        } else {
                            conn.query_drop("ROLLBACK").await.ok();
                        }
                    }
                } else if used_savepoint {
                    if let Some(arc) = MY_TX_CONN.with(|c| c.borrow().as_ref().cloned()) {
                        let mut conn = arc.lock().await;
                        let name = MY_SP_DEPTH.with(|d| {
                            let v = *d.borrow();
                            format!("sp{}", v)
                        });
                        if result.is_ok() {
                            conn.query_drop(format!("RELEASE SAVEPOINT {}", name))
                                .await
                                .ok();
                        } else {
                            conn.query_drop(format!("ROLLBACK TO SAVEPOINT {}", name))
                                .await
                                .ok();
                        }
                        MY_SP_DEPTH.with(|d| {
                            let mut b = d.borrow_mut();
                            if *b > 0 {
                                *b -= 1;
                            }
                        });
                    }
                }

                result
            };

            if not_initialized {
                MY_TX_CONN
                    .scope(std::cell::RefCell::new(None), async move {
                        MY_SP_DEPTH
                            .scope(std::cell::RefCell::new(0usize), run)
                            .await
                    })
                    .await
            } else {
                run.await
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::to_mysql_value;
        use mysql_async::Value;
        use storeit_core::ParamValue;

        #[test]
        fn to_mysql_value_maps_all_variants() {
            // String
            match to_mysql_value(ParamValue::String("s".to_string())) {
                Value::Bytes(b) => assert_eq!(b, b"s"),
                v => panic!("unexpected value for String: {:?}", v),
            }
            // I32
            match to_mysql_value(ParamValue::I32(32)) {
                Value::Int(i) => assert_eq!(i, 32),
                v => panic!("unexpected value for I32: {:?}", v),
            }
            // I64
            match to_mysql_value(ParamValue::I64(64)) {
                Value::Int(i) => assert_eq!(i, 64),
                v => panic!("unexpected value for I64: {:?}", v),
            }
            // F64
            match to_mysql_value(ParamValue::F64(6.5)) {
                Value::Double(f) => assert!((f - 6.5).abs() < 1e-10),
                v => panic!("unexpected value for F64: {:?}", v),
            }
            // Bool
            match to_mysql_value(ParamValue::Bool(true)) {
                Value::Int(i) => assert_eq!(i, 1),
                v => panic!("unexpected value for Bool(true): {:?}", v),
            }
            // Null
            match to_mysql_value(ParamValue::Null) {
                Value::NULL => {}
                v => panic!("unexpected value for Null: {:?}", v),
            }
        }
    }
}

#[cfg(feature = "mysql-async")]
pub use backend::{MysqlAsyncRepository, MysqlAsyncTransactionManager};
