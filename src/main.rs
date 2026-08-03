use std::{fs, net::SocketAddr, path::PathBuf};

use clap::{Parser, Subcommand};
use precinct_election_analysis_rs::{
    config::Config,
    export::build_bundle,
    ingestion::ingest_bytes,
    sample::sample_csv,
    web,
    workflow::{self, ALL_METHODS},
};

#[derive(Debug, Parser)]
#[command(name = "precinct-election-analysis-rs", version, about)]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Validate {
        input: PathBuf,
    },
    Analyze {
        input: PathBuf,
        #[arg(long, default_value = "candidate_a")]
        candidate: String,
        #[arg(long, value_delimiter = ',')]
        methods: Vec<String>,
        #[arg(long, default_value = "election-analysis-bundle.zip")]
        output: PathBuf,
    },
    Sample {
        #[arg(long, default_value_t = 120)]
        rows: usize,
        #[arg(long, default_value = "fictional-election-sample.csv")]
        output: PathBuf,
    },
    Serve {
        #[arg(long)]
        bind: Option<SocketAddr>,
    },
    Mcp,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    if matches!(cli.command, Command::Mcp) {
        return precinct_election_analysis_rs::mcp::run_stdio(cli.config).await;
    }
    let config = Config::load(cli.config.as_deref())?;
    match cli.command {
        Command::Validate { input } => {
            let result = ingest_bytes(&fs::read(&input)?, &input.display().to_string(), &config)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Analyze {
            input,
            candidate,
            methods,
            output,
        } => {
            let ingestion =
                ingest_bytes(&fs::read(&input)?, &input.display().to_string(), &config)?;
            let methods = if methods.is_empty() {
                ALL_METHODS
                    .iter()
                    .map(|method| (*method).to_owned())
                    .collect()
            } else {
                methods
            };
            let run = workflow::analyze(&ingestion, &candidate, &methods, &config)?;
            fs::write(&output, build_bundle(&ingestion, &run)?)?;
            println!(
                "Wrote {} records and {} method statuses to {}",
                run.analysis_rows,
                run.statuses.len(),
                output.display()
            );
        }
        Command::Sample { rows, output } => {
            if !(1..=100_000).contains(&rows) {
                anyhow::bail!("rows must be between 1 and 100000");
            }
            fs::write(&output, sample_csv(rows, config.statistics.random_seed))?;
            println!("Wrote fictional sample to {}", output.display());
        }
        Command::Serve { bind } => web::serve(config, bind).await?,
        Command::Mcp => unreachable!(),
    }
    Ok(())
}
