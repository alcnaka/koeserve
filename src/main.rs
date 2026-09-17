mod api;
mod error;
mod models;
mod playground;
mod tts;

use std::{net::SocketAddr, path::PathBuf, sync::Arc};

use anyhow::Context;
use axum::{Json, Router, routing::get};
use clap::{Args, Parser, Subcommand};
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

use crate::{api::ApiDoc, tts::TtsService};

#[derive(Clone)]
pub struct AppState { pub tts: Arc<TtsService> }

#[derive(Parser)]
#[command(name = "koeserve", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Start the HTTP TTS server (default when no command is given).
    Serve(ServeArgs),
    /// Manage downloadable HTS voice models.
    Models(ModelsArgs),
}

#[derive(Args, Default, Clone, Copy)]
struct ServeArgs {
    /// Expose /openapi.json.
    #[arg(long)]
    openapi: bool,

    /// Expose Swagger UI at /docs. Implies --openapi.
    #[arg(long)]
    docs: bool,

    /// Serve the embedded KoeServe Playground at /.
    #[arg(long)]
    playground: bool,
}

#[derive(Args)]
struct ModelsArgs {
    #[command(subcommand)]
    command: ModelsCommand,
}

#[derive(Subcommand)]
enum ModelsCommand {
    /// List configured voices, installation state, source and license.
    List,
    /// Download one or more voices/sources from the manifest.
    Fetch {
        /// Voice IDs or source IDs to fetch.
        ids: Vec<String>,
        /// Fetch all configured downloadable sources.
        #[arg(long)]
        all: bool,
        /// Permit a source without a pinned SHA-256. Not recommended.
        #[arg(long)]
        allow_unverified: bool,
    },
    /// Verify installed model files (SHA-256 when file_sha256 is configured).
    Verify,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| "koeserve=info,tower_http=info".into()))
        .init();

    let cli = Cli::parse();
    let config = models_config_path();
    match cli.command.unwrap_or(Command::Serve(ServeArgs::default())) {
        Command::Serve(args) => serve(config, args).await,
        Command::Models(args) => match args.command {
            ModelsCommand::List => models::list(&config),
            ModelsCommand::Fetch { ids, all, allow_unverified } => models::fetch(&config, &ids, all, allow_unverified).await,
            ModelsCommand::Verify => models::verify(&config),
        },
    }
}

fn models_config_path() -> PathBuf {
    PathBuf::from(std::env::var("KOESERVE_MODELS_CONFIG").unwrap_or_else(|_| "models/manifest.toml".to_owned()))
}

async fn serve(models_config: PathBuf, args: ServeArgs) -> anyhow::Result<()> {
    let workers = env_usize("KOESERVE_WORKERS", 2)?;
    let queue_capacity = env_usize("KOESERVE_QUEUE_CAPACITY", 32)?;
    let state = AppState { tts: Arc::new(TtsService::load(models_config, workers, queue_capacity)?) };

    let mut app = Router::new().merge(api::router());

    if args.docs {
        // SwaggerUi serves both /docs and the OpenAPI document it references.
        app = app.merge(SwaggerUi::new("/docs").url("/openapi.json", ApiDoc::openapi()));
    } else if args.openapi {
        app = app.route("/openapi.json", get(|| async { Json(ApiDoc::openapi()) }));
    }

    if args.playground {
        app = app.fallback(playground::static_asset);
    }

    let app = app
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = std::env::var("KOESERVE_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:3000".to_owned()).parse().context("invalid KOESERVE_ADDR")?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    info!(%addr, openapi = args.openapi || args.docs, docs = args.docs, playground = args.playground, "KoeServe listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

fn env_usize(name: &str, default: usize) -> anyhow::Result<usize> {
    match std::env::var(name) {
        Ok(value) => value.parse::<usize>().with_context(|| format!("invalid {name}")),
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(error) => Err(error.into()),
    }
}

async fn shutdown_signal() {
    let ctrl_c = async { tokio::signal::ctrl_c().await.expect("failed to install Ctrl+C handler"); };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler").recv().await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = terminate => {} }
}
