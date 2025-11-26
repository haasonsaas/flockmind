use anyhow::Result;
use clap::{Parser, Subcommand};
use flockmind::{create_raft_router, create_router, HiveDaemon, NodeConfig};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::net::TcpListener;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(name = "flockmind")]
#[command(about = "Distributed hive daemon with LLM brain")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Run {
        #[arg(short, long, default_value = "flockmind.toml")]
        config: PathBuf,
    },
    Init {
        #[arg(short, long, default_value = "flockmind.toml")]
        config: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "flockmind=info,openraft=warn".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { config: config_path } => {
            run_daemon(config_path).await?;
        }
        Commands::Init { config: config_path } => {
            init_config(config_path)?;
        }
    }

    Ok(())
}

async fn run_daemon(config_path: PathBuf) -> Result<()> {
    let config = if config_path.exists() {
        info!("Loading config from {:?}", config_path);
        NodeConfig::load(&config_path)?
    } else {
        info!("Config file not found, using defaults");
        NodeConfig::default()
    };

    let daemon = Arc::new(HiveDaemon::new(config.clone()).await?);

    let api_router = create_router(daemon.clone());
    let raft_router = create_raft_router(daemon.replicator().clone());
    let router = api_router.merge(raft_router);
    
    let listener = TcpListener::bind(&config.listen_addr()).await?;
    info!("API server listening on {}", config.listen_addr());

    let daemon_clone = daemon.clone();
    let api_handle = tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            error!("API server error: {}", e);
        }
    });

    let daemon_handle = tokio::spawn(async move {
        if let Err(e) = daemon_clone.run().await {
            error!("Daemon error: {}", e);
        }
    });

    tokio::signal::ctrl_c().await?;
    info!("Shutting down...");
    daemon.shutdown();

    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        async {
            let _ = daemon_handle.await;
            let _ = api_handle.await;
        },
    )
    .await;

    Ok(())
}

fn init_config(config_path: PathBuf) -> Result<()> {
    if config_path.exists() {
        anyhow::bail!("Config file already exists: {:?}", config_path);
    }

    let config = NodeConfig::default();
    config.save(&config_path)?;
    println!("Created config file: {:?}", config_path);
    println!("\nEdit the config file to:");
    println!("  - Set a unique node_id");
    println!("  - Configure peers for multi-node cluster");
    println!("  - Enable LLM brain with API key");
    println!("  - Adjust execution policy");

    Ok(())
}
