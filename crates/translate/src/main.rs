pub mod log;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    log::init();

    let cfg = match config::Config::load() {
        Ok(c) => c,
        Err(config::ConfigError::Io { .. }) => {
            // 配置文件缺失（首次运行）：用默认值
            tracing::info!("配置文件不存在，使用默认配置");
            config::Config::default()
        }
        // 解析失败说明配置已损坏：直接退出，避免默认配置掩盖根因
        Err(e) => return Err(e.into()),
    };
    let state = web::AppState::new(cfg.clone());
    let addr = format!("{}:{}", cfg.server.host, cfg.server.port);
    tracing::info!("translate web 服务启动: http://{addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, web::router(state)).await?;
    Ok(())
}
