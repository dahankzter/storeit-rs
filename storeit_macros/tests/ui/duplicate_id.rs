use storeit_macros::Entity;

#[derive(Entity)]
struct TwoIds {
    #[fetch(id)]
    id1: i64,
    #[fetch(id)]
    id2: i64,
}
