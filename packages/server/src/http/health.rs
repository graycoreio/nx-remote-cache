/// The server is up, which also means it can use its storage: it checks that before it starts
/// listening, and exits instead if it can't. No auth.
pub async fn health() -> &'static str {
    "OK"
}
