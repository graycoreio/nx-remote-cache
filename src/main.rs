use std::{path::PathBuf, process::ExitCode};

use nx_remote_cache::{
    auth::static_token::hash_token,
    config::{Config, DEFAULT_CONFIG_PATH},
    http::{AppState, app},
};
use tower::make::Shared;
use tracing_subscriber::EnvFilter;

const USAGE: &str = "\
usage:
  nx-remote-cache [serve] [--config <path>]   run the server (default config: ./config.toml)
  nx-remote-cache generate-token              print a new random static token and its sha256
  nx-remote-cache hash-token <token>          print the sha256 to put in a static token entry";

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["generate-token"] => {
            // Two v4 UUIDs: 244 random bits, hex-encoded, no characters that need quoting.
            let token = format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            );
            println!("token:  {token}\nsha256: {}", hash_token(&token));
            ExitCode::SUCCESS
        }
        ["hash-token", token] => {
            println!("{}", hash_token(token));
            ExitCode::SUCCESS
        }
        [] | ["serve"] => serve(PathBuf::from(DEFAULT_CONFIG_PATH)).await,
        ["--config", path] | ["serve", "--config", path] => serve(PathBuf::from(path)).await,
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

async fn serve(config_path: PathBuf) -> ExitCode {
    let config = match Config::load(&config_path) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    init_tracing(config.server.json_logs);

    match run(config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

async fn run(config: Config) -> Result<(), String> {
    let store = config.build_store()?;
    // Fail fast: a server that can't reach its storage would only produce cache misses.
    store
        .ready()
        .await
        .map_err(|e| format!("storage is not usable: {e}"))?;
    let auth = config.build_auth().await?;

    let state = AppState {
        store,
        auth: auth.chain,
        max_artifact_bytes: config.server.max_artifact_bytes,
        #[cfg(feature = "mint")]
        minter: auth.minter,
    };
    let listener = tokio::net::TcpListener::bind(config.server.listen)
        .await
        .map_err(|e| format!("cannot listen on {}: {e}", config.server.listen))?;
    tracing::info!(addr = %config.server.listen, "listening");
    axum::serve(listener, Shared::new(app(state)))
        .with_graceful_shutdown(shutdown_signal())
        .await
        .map_err(|e| format!("server error: {e}"))
}

fn init_tracing(json: bool) {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let builder = tracing_subscriber::fmt().with_env_filter(filter);
    if json {
        builder.json().init();
    } else {
        builder.init();
    }
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
    tracing::info!("shutting down");
}
