#![allow(unexpected_cfgs)]
use storeit::transactions::{default_transaction_template, TransactionContext};
use storeit::Entity;

#[derive(Entity, Clone, Debug, PartialEq)]
struct E1 {
    #[fetch(id)]
    id: Option<i64>,
}

#[derive(Entity, Clone, Debug, PartialEq)]
struct E2 {
    #[fetch(id)]
    id: Option<i64>,
}

#[tokio::test]
async fn single_template_used_for_multiple_entities() {
    // Obtain a single, entity-agnostic template.
    let tpl = default_transaction_template();

    // Use it for a closure that mentions two different entities.
    let out = tpl
        .execute(|_ctx: TransactionContext| async move {
            // No DB here; we just prove the template is independent of entity types.
            let _a = E1 { id: Some(1) };
            let _b = E2 { id: Some(2) };
            Ok::<_, storeit::RepoError>(_a.id.unwrap() + _b.id.unwrap())
        })
        .await
        .expect("template should execute and return a value");

    assert_eq!(out, 3);
}
