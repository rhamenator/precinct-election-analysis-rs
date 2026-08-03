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
    run(Cli::parse()).await
}

async fn run(cli: Cli) -> anyhow::Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn command_line_parses_defaults_and_rejects_missing_commands() {
        let cli = Cli::try_parse_from(["app", "sample"]).unwrap();
        match cli.command {
            Command::Sample { rows, output } => {
                assert_eq!(rows, 120);
                assert_eq!(output, PathBuf::from("fictional-election-sample.csv"));
            }
            _ => panic!("expected sample command"),
        }
        assert!(Cli::try_parse_from(["app"]).is_err());
    }

    #[tokio::test]
    async fn sample_validate_and_analyze_commands_complete_real_file_workflows() {
        let directory = tempdir().unwrap();
        let sample = directory.path().join("sample.csv");
        run(Cli {
            config: None,
            command: Command::Sample {
                rows: 25,
                output: sample.clone(),
            },
        })
        .await
        .unwrap();
        assert_eq!(fs::read_to_string(&sample).unwrap().lines().count(), 26);

        run(Cli {
            config: None,
            command: Command::Validate {
                input: sample.clone(),
            },
        })
        .await
        .unwrap();

        for (methods, filename) in [
            (Vec::new(), "all.zip"),
            (vec!["vote_share_by_count".into()], "selected.zip"),
        ] {
            let output = directory.path().join(filename);
            run(Cli {
                config: None,
                command: Command::Analyze {
                    input: sample.clone(),
                    candidate: "candidate_a".into(),
                    methods,
                    output: output.clone(),
                },
            })
            .await
            .unwrap();
            assert!(fs::metadata(output).unwrap().len() > 0);
        }
    }

    #[tokio::test]
    async fn commands_propagate_validation_and_io_failures() {
        let directory = tempdir().unwrap();
        for rows in [0, 100_001] {
            let result = run(Cli {
                config: None,
                command: Command::Sample {
                    rows,
                    output: directory.path().join("invalid.csv"),
                },
            })
            .await;
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("between 1 and 100000")
            );
        }

        let result = run(Cli {
            config: None,
            command: Command::Validate {
                input: directory.path().join("missing.csv"),
            },
        })
        .await;
        assert!(result.is_err());
    }
}
