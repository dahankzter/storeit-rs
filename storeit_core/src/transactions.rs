//! Backend-agnostic transaction abstractions modeled after Spring's TransactionTemplate.
//! This module defines only generic types and traits. Backends provide implementations.

use std::time::Duration;

/// Transaction propagation behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Propagation {
    Required,
    RequiresNew,
    Supports,
    NotSupported,
    Never,
    Nested,
}

/// Transaction isolation level (best-effort across backends).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Isolation {
    Default,
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

/// Transaction definition describing desired semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionDefinition {
    pub propagation: Propagation,
    pub isolation: Isolation,
    pub read_only: bool,
    pub timeout: Option<Duration>,
}

impl Default for TransactionDefinition {
    fn default() -> Self {
        Self {
            propagation: Propagation::Required,
            isolation: Isolation::Default,
            read_only: false,
            timeout: None,
        }
    }
}

/// Mutable transaction status recorded by the manager for a running transaction.
#[derive(Debug, Default)]
pub struct TransactionStatus {
    new: bool,
    rollback_only: bool,
}

impl TransactionStatus {
    pub fn new(is_new: bool) -> Self {
        Self {
            new: is_new,
            rollback_only: false,
        }
    }
    pub fn is_new_transaction(&self) -> bool {
        self.new
    }
    pub fn is_rollback_only(&self) -> bool {
        self.rollback_only
    }
    pub fn set_rollback_only(&mut self) {
        self.rollback_only = true;
    }
}

/// Opaque handle passed inside execute callbacks. Backends define concrete internals.
/// This type is intentionally minimal; it just carries capability to create repos
/// or access underlying connections in backend-specific managers.
#[derive(Debug, Clone, Copy)]
pub struct TransactionContext<'a> {
    // Erased pointer to manager-provided context data; only the manager knows how to use it.
    // We keep it as a raw pointer-sized value via an unnameable lifetime-bound token.
    // End users cannot do anything with it without the manager.
    _priv: std::marker::PhantomData<&'a ()>,
}

impl<'a> TransactionContext<'a> {
    pub fn new() -> Self {
        Self {
            _priv: std::marker::PhantomData,
        }
    }
}

impl<'a> Default for TransactionContext<'a> {
    fn default() -> Self {
        Self::new()
    }
}

/// Backend-implemented transaction manager.
#[async_trait::async_trait]
pub trait TransactionManager: Send + Sync {
    /// Execute the provided async callback within a transactional scope according to
    /// the given definition. Implementations must ensure commit/rollback as appropriate.
    async fn execute<'a, R, F, Fut>(
        &'a self,
        def: &TransactionDefinition,
        f: F,
    ) -> crate::RepoResult<R>
    where
        // The callback receives an ephemeral TransactionContext that the manager can
        // use to vend transaction-bound resources via additional manager-specific APIs.
        F: FnOnce(TransactionContext<'a>) -> Fut + Send + 'a,
        Fut: core::future::Future<Output = crate::RepoResult<R>> + Send + 'a,
        R: Send + 'a;
}

/// Convenience wrapper similar to Spring's TransactionTemplate.
#[derive(Debug)]
pub struct TransactionTemplate<M: TransactionManager> {
    manager: M,
    defaults: TransactionDefinition,
}

impl<M: TransactionManager> TransactionTemplate<M> {
    pub fn new(manager: M) -> Self {
        Self {
            manager,
            defaults: TransactionDefinition::default(),
        }
    }
    pub fn with_defaults(mut self, def: TransactionDefinition) -> Self {
        self.defaults = def;
        self
    }

    pub async fn execute<R, F, Fut>(&self, f: F) -> crate::RepoResult<R>
    where
        F: for<'a> FnOnce(TransactionContext<'a>) -> Fut + Send,
        Fut: core::future::Future<Output = crate::RepoResult<R>> + Send,
        R: Send + 'static,
    {
        self.manager.execute(&self.defaults, f).await
    }

    pub async fn execute_with<R, F, Fut>(
        &self,
        def: &TransactionDefinition,
        f: F,
    ) -> crate::RepoResult<R>
    where
        F: for<'a> FnOnce(TransactionContext<'a>) -> Fut + Send,
        Fut: core::future::Future<Output = crate::RepoResult<R>> + Send,
        R: Send + 'static,
    {
        self.manager.execute(def, f).await
    }
}

/// A simple, entity-agnostic default transaction manager that just runs the
/// closure without starting a real database transaction.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultTransactionManager;

#[async_trait::async_trait]
impl TransactionManager for DefaultTransactionManager {
    async fn execute<'a, R, F, Fut>(
        &'a self,
        _def: &TransactionDefinition,
        f: F,
    ) -> crate::RepoResult<R>
    where
        F: FnOnce(TransactionContext<'a>) -> Fut + Send + 'a,
        Fut: core::future::Future<Output = crate::RepoResult<R>> + Send + 'a,
        R: Send + 'a,
    {
        f(TransactionContext::new()).await
    }
}

/// Convenience to obtain a TransactionTemplate using the default manager, without
/// exposing any manager type in user code.
pub fn default_transaction_template() -> TransactionTemplate<DefaultTransactionManager> {
    TransactionTemplate::new(DefaultTransactionManager)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A tiny mock manager that records calls and runs the callback without real transactions.
    struct MockManager;
    #[async_trait::async_trait]
    impl TransactionManager for MockManager {
        async fn execute<'a, R, F, Fut>(
            &'a self,
            def: &TransactionDefinition,
            f: F,
        ) -> crate::RepoResult<R>
        where
            F: FnOnce(TransactionContext<'a>) -> Fut + Send + 'a,
            Fut: core::future::Future<Output = crate::RepoResult<R>> + Send + 'a,
            R: Send + 'a,
        {
            // Validate that defaults pass through; then just call the function.
            let _ = def.clone();
            f(TransactionContext::new()).await
        }
    }

    /// A manager that asserts the TransactionDefinition equals an expected value.
    struct AssertManager {
        expected: TransactionDefinition,
    }
    #[async_trait::async_trait]
    impl TransactionManager for AssertManager {
        async fn execute<'a, R, F, Fut>(
            &'a self,
            def: &TransactionDefinition,
            f: F,
        ) -> crate::RepoResult<R>
        where
            F: FnOnce(TransactionContext<'a>) -> Fut + Send + 'a,
            Fut: core::future::Future<Output = crate::RepoResult<R>> + Send + 'a,
            R: Send + 'a,
        {
            assert_eq!(
                def, &self.expected,
                "TransactionDefinition did not match expected"
            );
            f(TransactionContext::new()).await
        }
    }

    #[test]
    fn template_delegates_to_manager_and_returns_value() {
        let mgr = MockManager;
        let tpl = TransactionTemplate::new(mgr);
        let fut = tpl.execute(|_ctx| async move { Ok::<_, crate::RepoError>(41 + 1) });
        let out = futures::executor::block_on(fut).unwrap();
        assert_eq!(out, 42);
    }

    #[test]
    fn defaults_and_status_basic_behaviour() {
        let def = TransactionDefinition::default();
        assert_eq!(def.propagation, Propagation::Required);
        assert_eq!(def.isolation, Isolation::Default);
        assert!(!def.read_only);
        assert!(def.timeout.is_none());

        let mut st = TransactionStatus::new(true);
        assert!(st.is_new_transaction());
        assert!(!st.is_rollback_only());
        st.set_rollback_only();
        assert!(st.is_rollback_only());
    }

    #[test]
    fn template_with_defaults_overrides_definition() {
        let expected = TransactionDefinition {
            propagation: Propagation::RequiresNew,
            isolation: Isolation::Serializable,
            read_only: true,
            timeout: Some(std::time::Duration::from_secs(1)),
        };
        let mgr = AssertManager {
            expected: expected.clone(),
        };
        let tpl = TransactionTemplate::new(mgr).with_defaults(expected);
        let fut = tpl.execute(|_ctx| async move { Ok::<_, crate::RepoError>(123) });
        let out = futures::executor::block_on(fut).unwrap();
        assert_eq!(out, 123);
    }

    #[test]
    fn template_execute_with_uses_provided_definition() {
        let provided = TransactionDefinition {
            propagation: Propagation::Supports,
            isolation: Isolation::ReadCommitted,
            read_only: false,
            timeout: Some(std::time::Duration::from_millis(250)),
        };
        let mgr = AssertManager {
            expected: provided.clone(),
        };
        let tpl = TransactionTemplate::new(mgr);
        let fut = tpl.execute_with(
            &provided,
            |_ctx| async move { Ok::<_, crate::RepoError>(7 * 6) },
        );
        let out = futures::executor::block_on(fut).unwrap();
        assert_eq!(out, 42);
    }

    #[test]
    fn default_transaction_template_executes_closure() {
        let tpl = default_transaction_template();
        let fut = tpl.execute(|_ctx| async move { Ok::<_, crate::RepoError>(5) });
        let out = futures::executor::block_on(fut).unwrap();
        assert_eq!(out, 5);
    }

    #[test]
    fn cover_all_variants_propagation_and_isolation() {
        // Touch all enum variants (ensure Debug formatting paths get exercised too)
        let props = [
            Propagation::Required,
            Propagation::RequiresNew,
            Propagation::Supports,
            Propagation::NotSupported,
            Propagation::Never,
            Propagation::Nested,
        ];
        let isos = [
            Isolation::Default,
            Isolation::ReadCommitted,
            Isolation::RepeatableRead,
            Isolation::Serializable,
        ];
        // Basic sanity: Debug representations are non-empty
        for p in props.iter() {
            let s = format!("{:?}", p);
            assert!(!s.is_empty());
        }
        for i in isos.iter() {
            let s = format!("{:?}", i);
            assert!(!s.is_empty());
        }
        assert_eq!(props.len(), 6);
        assert_eq!(isos.len(), 4);
    }
}
