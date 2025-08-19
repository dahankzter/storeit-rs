use storeit_macros::Entity;

#[derive(Entity)]
struct NoId {
    email: String,
}
