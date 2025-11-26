use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::Value;

#[derive(Parser)]
#[command(name = "flockctl")]
#[command(about = "CLI for FlockMind hive management")]
struct Cli {
    #[arg(short, long, default_value = "http://127.0.0.1:9000")]
    addr: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Status,
    Cluster,

    #[command(subcommand)]
    Task(TaskCommands),

    #[command(subcommand)]
    Goal(GoalCommands),

    Attachments,
}

#[derive(Subcommand)]
enum TaskCommands {
    List,
    Submit {
        #[arg(short, long)]
        node: String,

        #[arg(short, long)]
        echo: Option<String>,

        #[arg(long)]
        check_service: Option<String>,

        #[arg(short, long, default_value = "5")]
        priority: u8,
    },
}

#[derive(Subcommand)]
enum GoalCommands {
    List,
    Add {
        #[arg(short, long)]
        description: String,

        #[arg(short, long, default_value = "5")]
        priority: u8,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = reqwest::Client::new();
    let base_url = cli.addr;

    match cli.command {
        Commands::Status => {
            let resp: Value = client
                .get(format!("{}/status", base_url))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Cluster => {
            let resp: Value = client
                .get(format!("{}/cluster", base_url))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Commands::Task(cmd) => match cmd {
            TaskCommands::List => {
                let resp: Value = client
                    .get(format!("{}/tasks", base_url))
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            TaskCommands::Submit {
                node,
                echo,
                check_service,
                priority,
            } => {
                let payload = if let Some(msg) = echo {
                    serde_json::json!({
                        "Echo": { "message": msg }
                    })
                } else if let Some(svc) = check_service {
                    serde_json::json!({
                        "CheckService": { "service_name": svc }
                    })
                } else {
                    anyhow::bail!("Specify --echo or --check-service");
                };

                let body = serde_json::json!({
                    "target_node": node,
                    "payload": payload,
                    "priority": priority,
                });

                let resp: Value = client
                    .post(format!("{}/tasks", base_url))
                    .json(&body)
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
        },
        Commands::Goal(cmd) => match cmd {
            GoalCommands::List => {
                let resp: Value = client
                    .get(format!("{}/goals", base_url))
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
            GoalCommands::Add {
                description,
                priority,
            } => {
                let body = serde_json::json!({
                    "description": description,
                    "priority": priority,
                });

                let resp: Value = client
                    .post(format!("{}/goals", base_url))
                    .json(&body)
                    .send()
                    .await?
                    .json()
                    .await?;
                println!("{}", serde_json::to_string_pretty(&resp)?);
            }
        },
        Commands::Attachments => {
            let resp: Value = client
                .get(format!("{}/attachments", base_url))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
    }

    Ok(())
}
