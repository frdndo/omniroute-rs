use omniroute_core::proxy;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let port = std::env::var("OMNIROUTE_PORT")
        .ok()
        .and_then(|p| p.parse::<u16>().ok())
        .unwrap_or(20128);
    let version = env!("CARGO_PKG_VERSION");
    tracing::info!("🚀 omniroute-rs proxy v{} starting on :{}", version, port);
    proxy::start_server(port, version).await;
}
