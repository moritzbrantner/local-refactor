#[tokio::main]
async fn main() -> anyhow::Result<()> {
    local_refactor_service::serve_from_env().await
}
