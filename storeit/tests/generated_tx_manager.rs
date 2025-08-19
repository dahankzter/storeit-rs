#![allow(unexpected_cfgs)]
use storeit::transactions::{default_transaction_template, TransactionContext};
use storeit::Entity;

#[derive(Entity, Clone, Debug, PartialEq)]
struct TxDemoEntity {
    #[fetch(id)]
    id: Option<i64>,
}

#[tokio::test]
async fn generated_tx_manager_executes_closure() {
    // Use the default backend-agnostic template for this test.
    let tpl = default_transaction_template();
    let out = tpl
        .execute(|_ctx: TransactionContext| async move { Ok::<_, storeit::RepoError>(40 + 2) })
        .await
        .expect("tx execute should succeed");
    assert_eq!(out, 42);
}
