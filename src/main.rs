use anyhow::{bail, Context, Result};
use clap::Parser;
use colored::Colorize;
use std::net::SocketAddr;
use tcptap::throttle::parse_rate_limit;
use tcptap::{start_proxy, Config, OutputFormat};

#[derive(Parser, Debug)]
#[command(
    name = "tcptap",
    author = "Animesh Raj <animesh080808@gmail.com>",
    version,
    about = "A zero-daemon TCP tap and chaos proxy — duplex inspection, latency injection, bandwidth throttling, and socket severing",
    after_help = "EXAMPLES:\n  \
      tcptap --listen 127.0.0.1:5433 --target 127.0.0.1:5432 --format hex\n  \
      tcptap --listen 127.0.0.1:9000 --target 127.0.0.1:6379 --delay-ms 100 --jitter-ms 25\n  \
      tcptap --listen 127.0.0.1:8000 --target 127.0.0.1:8080 --rate-limit 50KB/s\n  \
      tcptap --listen 127.0.0.1:7000 --target 127.0.0.1:7001 --corrupt-rate 0.05 --drop-rate 0.02"
)]
struct Cli {
    /// Socket address to listen on
    #[arg(short, long, default_value = "127.0.0.1:9000")]
    listen: SocketAddr,

    /// Target upstream socket address to forward traffic to
    #[arg(short, long)]
    target: SocketAddr,

    /// Output inspection format: 'hex' (hexdump), 'text' (ASCII/UTF-8), or 'none'
    #[arg(short, long, value_enum, default_value_t = OutputFormat::Hex)]
    format: OutputFormat,

    /// Artificial latency delay to inject in milliseconds
    #[arg(long, default_value_t = 0)]
    delay_ms: u64,

    /// Random latency jitter in milliseconds (uniform in +/- jitter_ms)
    #[arg(long, default_value_t = 0)]
    jitter_ms: u64,

    /// Bandwidth throttling rate limit (e.g. '100KB/s', '1MB/s', '500B/s')
    #[arg(long)]
    rate_limit: Option<String>,

    /// Packet corruption probability rate (0.0 to 1.0)
    #[arg(long, default_value_t = 0.0)]
    corrupt_rate: f64,

    /// Packet drop probability rate (0.0 to 1.0)
    #[arg(long, default_value_t = 0.0)]
    drop_rate: f64,

    /// Abruptly sever connection after transferring this many total bytes
    #[arg(long)]
    close_after_bytes: Option<usize>,

    /// Abruptly sever connection after this many seconds
    #[arg(long)]
    close_after_secs: Option<u64>,

    /// Sever connections abruptly with TCP RST
    #[arg(long)]
    reset: bool,

    /// Suppress connection status messages
    #[arg(short, long)]
    quiet: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    if cli.corrupt_rate < 0.0 || cli.corrupt_rate > 1.0 {
        bail!("--corrupt-rate must be between 0.0 and 1.0");
    }
    if cli.drop_rate < 0.0 || cli.drop_rate > 1.0 {
        bail!("--drop-rate must be between 0.0 and 1.0");
    }

    let rate_limit_bytes = if let Some(ref r) = cli.rate_limit {
        Some(parse_rate_limit(r).with_context(|| format!("Invalid --rate-limit '{}'", r))?)
    } else {
        None
    };

    let config = Config {
        listen: cli.listen,
        target: cli.target,
        format: cli.format,
        delay_ms: cli.delay_ms,
        jitter_ms: cli.jitter_ms,
        rate_limit_bytes_per_sec: rate_limit_bytes,
        corrupt_rate: cli.corrupt_rate,
        drop_rate: cli.drop_rate,
        close_after_bytes: cli.close_after_bytes,
        close_after_secs: cli.close_after_secs,
        reset: cli.reset,
        quiet: cli.quiet,
    };

    let server = start_proxy(config).await?;

    if !cli.quiet {
        eprintln!(
            "{} listening on {} -> forwarding to {}",
            "tcptap".bold().green(),
            server.local_addr().to_string().cyan(),
            cli.target.to_string().yellow()
        );
        if cli.delay_ms > 0 || cli.jitter_ms > 0 {
            eprintln!(
                "{} Delay: {}ms (jitter: +/-{}ms)",
                "  Chaos:".dimmed(),
                cli.delay_ms,
                cli.jitter_ms
            );
        }
        if let Some(limit) = rate_limit_bytes {
            eprintln!(
                "{} Bandwidth throttled to {} B/s",
                "  Throttle:".dimmed(),
                limit
            );
        }
        eprintln!("Press Ctrl+C to shut down.");
    }

    tokio::signal::ctrl_c().await.context("Failed to wait for Ctrl+C")?;

    if !cli.quiet {
        eprintln!("\n{}", "Shutting down tcptap...".yellow());
    }
    server.shutdown();
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    Ok(())
}
