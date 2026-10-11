pub mod chaos;
pub mod format;
pub mod throttle;

use anyhow::{Context, Result};
use chaos::ChaosConfig;
use clap::ValueEnum;
use colored::Colorize;
use format::{format_text, hexdump};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use throttle::RateLimiter;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Hex,
    Text,
    None,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub listen: SocketAddr,
    pub target: SocketAddr,
    pub format: OutputFormat,
    pub delay_ms: u64,
    pub jitter_ms: u64,
    pub rate_limit_bytes_per_sec: Option<u64>,
    pub corrupt_rate: f64,
    pub drop_rate: f64,
    pub close_after_bytes: Option<usize>,
    pub close_after_secs: Option<u64>,
    pub reset: bool,
    pub quiet: bool,
}

pub struct ServerHandle {
    local_addr: SocketAddr,
    shutdown_tx: watch::Sender<bool>,
}

impl ServerHandle {
    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

pub async fn start_proxy(config: Config) -> Result<ServerHandle> {
    let listener = TcpListener::bind(config.listen)
        .await
        .with_context(|| format!("Failed to bind to {}", config.listen))?;
    let local_addr = listener.local_addr()?;

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    let config = Arc::new(config);
    let active_connections = Arc::new(AtomicUsize::new(0));

    let c_cfg = Arc::clone(&config);
    let c_conns = Arc::clone(&active_connections);

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    if *shutdown_rx.borrow() {
                        break;
                    }
                }
                accept_res = listener.accept() => {
                    match accept_res {
                        Ok((client_stream, client_addr)) => {
                            let cfg = Arc::clone(&c_cfg);
                            let conns = Arc::clone(&c_conns);
                            let s_rx = shutdown_rx.clone();

                            tokio::spawn(async move {
                                conns.fetch_add(1, Ordering::Relaxed);
                                if !cfg.quiet {
                                    eprintln!(
                                        "{} Client {} connected (active: {})",
                                        "tcptap".bold().green(),
                                        client_addr,
                                        conns.load(Ordering::Relaxed)
                                    );
                                }

                                if let Err(e) = handle_duplex(client_stream, client_addr, cfg.clone(), s_rx).await {
                                    if !cfg.quiet {
                                        eprintln!("{} Error on {}: {}", "tcptap".bold().red(), client_addr, e);
                                    }
                                }

                                conns.fetch_sub(1, Ordering::Relaxed);
                                if !cfg.quiet {
                                    eprintln!(
                                        "{} Client {} disconnected (active: {})",
                                        "tcptap".bold().yellow(),
                                        client_addr,
                                        conns.load(Ordering::Relaxed)
                                    );
                                }
                            });
                        }
                        Err(e) => {
                            if !c_cfg.quiet {
                                eprintln!("{} Accept error: {}", "tcptap".bold().red(), e);
                            }
                        }
                    }
                }
            }
        }
    });

    Ok(ServerHandle {
        local_addr,
        shutdown_tx,
    })
}

async fn handle_duplex(
    client_stream: TcpStream,
    client_addr: SocketAddr,
    config: Arc<Config>,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<()> {
    let target_stream = TcpStream::connect(config.target)
        .await
        .with_context(|| format!("Failed to connect to target {}", config.target))?;

    let (mut client_r, mut client_w) = client_stream.into_split();
    let (mut target_r, mut target_w) = target_stream.into_split();

    let total_bytes_client = Arc::new(AtomicUsize::new(0));
    let total_bytes_target = Arc::new(AtomicUsize::new(0));

    let chaos = ChaosConfig {
        delay_ms: config.delay_ms,
        jitter_ms: config.jitter_ms,
        corrupt_rate: config.corrupt_rate,
        drop_rate: config.drop_rate,
    };

    // Client -> Target task
    let c2t_cfg = Arc::clone(&config);
    let c2t_bytes = Arc::clone(&total_bytes_client);
    let c2t_chaos = chaos.clone();
    let mut c2t_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        let mut limiter = c2t_cfg.rate_limit_bytes_per_sec.map(RateLimiter::new);

        loop {
            let n = match client_r.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };

            let mut chunk = buf[..n].to_vec();

            if c2t_chaos.should_drop() {
                if !c2t_cfg.quiet {
                    eprintln!("{} [>>>] Dropped {} bytes", "tcptap chaos".bold().red(), n);
                }
                continue;
            }

            c2t_chaos.apply_delay().await;
            if c2t_chaos.maybe_corrupt(&mut chunk) && !c2t_cfg.quiet {
                eprintln!("{} [>>>] Corrupted byte in payload", "tcptap chaos".bold().red());
            }

            if let Some(ref mut lim) = limiter {
                lim.throttle(chunk.len()).await;
            }

            let (chunk_to_send, reached_limit) = if let Some(limit) = c2t_cfg.close_after_bytes {
                let current = c2t_bytes.load(Ordering::Relaxed);
                if current >= limit {
                    break;
                }
                let remaining = limit - current;
                if chunk.len() >= remaining {
                    (chunk[..remaining].to_vec(), true)
                } else {
                    (chunk, false)
                }
            } else {
                (chunk, false)
            };

            c2t_bytes.fetch_add(chunk_to_send.len(), Ordering::Relaxed);
            print_traffic(c2t_cfg.format, ">>>", client_addr, &chunk_to_send);

            if target_w.write_all(&chunk_to_send).await.is_err() {
                break;
            }
            let _ = target_w.flush().await;

            if reached_limit {
                if !c2t_cfg.quiet {
                    eprintln!("{} Byte limit reached, severing connection", "tcptap".bold().yellow());
                }
                break;
            }
        }
        let _ = target_w.shutdown().await;
    });

    // Target -> Client task
    let t2c_cfg = Arc::clone(&config);
    let t2c_bytes = Arc::clone(&total_bytes_target);
    let t2c_chaos = chaos.clone();
    let mut t2c_task = tokio::spawn(async move {
        let mut buf = vec![0u8; 8192];
        let mut limiter = t2c_cfg.rate_limit_bytes_per_sec.map(RateLimiter::new);

        loop {
            let n = match target_r.read(&mut buf).await {
                Ok(0) => break,
                Ok(n) => n,
                Err(_) => break,
            };

            let mut chunk = buf[..n].to_vec();

            if t2c_chaos.should_drop() {
                if !t2c_cfg.quiet {
                    eprintln!("{} [<<<] Dropped {} bytes", "tcptap chaos".bold().red(), n);
                }
                continue;
            }

            t2c_chaos.apply_delay().await;
            if t2c_chaos.maybe_corrupt(&mut chunk) && !t2c_cfg.quiet {
                eprintln!("{} [<<<] Corrupted byte in payload", "tcptap chaos".bold().red());
            }

            if let Some(ref mut lim) = limiter {
                lim.throttle(chunk.len()).await;
            }

            let (chunk_to_send, reached_limit) = if let Some(limit) = t2c_cfg.close_after_bytes {
                let current = t2c_bytes.load(Ordering::Relaxed);
                if current >= limit {
                    break;
                }
                let remaining = limit - current;
                if chunk.len() >= remaining {
                    (chunk[..remaining].to_vec(), true)
                } else {
                    (chunk, false)
                }
            } else {
                (chunk, false)
            };

            t2c_bytes.fetch_add(chunk_to_send.len(), Ordering::Relaxed);
            print_traffic(t2c_cfg.format, "<<<", client_addr, &chunk_to_send);

            if client_w.write_all(&chunk_to_send).await.is_err() {
                break;
            }
            let _ = client_w.flush().await;

            if reached_limit {
                if !t2c_cfg.quiet {
                    eprintln!("{} Byte limit reached, severing connection", "tcptap".bold().yellow());
                }
                break;
            }
        }
        let _ = client_w.shutdown().await;
    });

    // Timeout supervision if close_after_secs is set
    let timer_fut = async {
        if let Some(secs) = config.close_after_secs {
            tokio::time::sleep(Duration::from_secs(secs)).await;
            true
        } else {
            std::future::pending::<bool>().await
        }
    };

    tokio::select! {
        _ = shutdown_rx.changed() => {}
        _ = timer_fut => {
            if !config.quiet {
                eprintln!("{} Duration limit reached, severing connection", "tcptap".bold().yellow());
            }
        }
        _ = &mut c2t_task => {}
        _ = &mut t2c_task => {}
    }

    c2t_task.abort();
    t2c_task.abort();

    Ok(())
}

fn print_traffic(format: OutputFormat, dir: &'static str, addr: SocketAddr, data: &[u8]) {
    match format {
        OutputFormat::None => {}
        OutputFormat::Hex => {
            let header = if dir == ">>>" {
                format!("{} Client {} ({} bytes)", dir.bold().cyan(), addr, data.len())
            } else {
                format!("{} Upstream ({} bytes)", dir.bold().green(), data.len())
            };
            println!("{}", header);
            print!("{}", hexdump(data));
        }
        OutputFormat::Text => {
            let formatted = format_text(data);
            if dir == ">>>" {
                println!("{} {}", dir.bold().cyan(), formatted);
            } else {
                println!("{} {}", dir.bold().green(), formatted);
            }
        }
    }
}
