use anyhow::Result;
use clap::Parser;
use std::process::ExitCode;
use tagent::cli::Cli;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    match run().await {
        Ok(code) => code,
        Err(err) => {
            // Errors are built without request headers or key values; the
            // redaction layer also strips known secrets from tool output.
            eprintln!("ta: {err:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<ExitCode> {
    Cli::parse().run().await
}
