#[tokio::main]
async fn main() -> anyhow::Result<()> {
    mlx_ollama_daemon::run().await
}
