use std::net::SocketAddr;
use std::time::{Duration, Instant};
use tcptap::{start_proxy, Config, OutputFormat};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

async fn spawn_echo_server() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut buf = vec![0u8; 4096];
                loop {
                    match stream.read(&mut buf).await {
                        Ok(0) => break,
                        Ok(n) => {
                            if stream.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            });
        }
    });

    addr
}

#[tokio::test]
async fn test_duplex_passthrough() {
    let echo_addr = spawn_echo_server().await;

    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: echo_addr,
        format: OutputFormat::Hex,
        delay_ms: 0,
        jitter_ms: 0,
        rate_limit_bytes_per_sec: None,
        corrupt_rate: 0.0,
        drop_rate: 0.0,
        close_after_bytes: None,
        close_after_secs: None,
        reset: false,
        quiet: true,
    };

    let proxy = start_proxy(config).await.expect("Failed to start tcptap");
    let proxy_addr = proxy.local_addr();

    let mut client = TcpStream::connect(proxy_addr).await.expect("connect failed");

    let payload = b"Hello from tcptap integration test!";
    client.write_all(payload).await.expect("write failed");

    let mut resp = vec![0u8; payload.len()];
    client.read_exact(&mut resp).await.expect("read failed");

    assert_eq!(&resp, payload);
    proxy.shutdown();
}

#[tokio::test]
async fn test_latency_injection() {
    let echo_addr = spawn_echo_server().await;

    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: echo_addr,
        format: OutputFormat::None,
        delay_ms: 80,
        jitter_ms: 0,
        rate_limit_bytes_per_sec: None,
        corrupt_rate: 0.0,
        drop_rate: 0.0,
        close_after_bytes: None,
        close_after_secs: None,
        reset: false,
        quiet: true,
    };

    let proxy = start_proxy(config).await.expect("Failed to start tcptap");
    let proxy_addr = proxy.local_addr();

    let mut client = TcpStream::connect(proxy_addr).await.expect("connect failed");

    let start = Instant::now();
    let payload = b"ping";
    client.write_all(payload).await.expect("write failed");

    let mut resp = vec![0u8; payload.len()];
    client.read_exact(&mut resp).await.expect("read failed");
    let elapsed = start.elapsed();

    // Round-trip delay = client->target (80ms) + target->client (80ms) = ~160ms
    assert!(
        elapsed >= Duration::from_millis(150),
        "Expected >= 150ms round-trip delay, got {:?}",
        elapsed
    );

    proxy.shutdown();
}

#[tokio::test]
async fn test_bandwidth_throttling() {
    let echo_addr = spawn_echo_server().await;

    // Rate limit to 10 KB/s
    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: echo_addr,
        format: OutputFormat::None,
        delay_ms: 0,
        jitter_ms: 0,
        rate_limit_bytes_per_sec: Some(10 * 1024),
        corrupt_rate: 0.0,
        drop_rate: 0.0,
        close_after_bytes: None,
        close_after_secs: None,
        reset: false,
        quiet: true,
    };

    let proxy = start_proxy(config).await.expect("Failed to start tcptap");
    let proxy_addr = proxy.local_addr();

    let mut client = TcpStream::connect(proxy_addr).await.expect("connect failed");

    // Send 10KB
    let payload = vec![0x42; 10 * 1024];
    let start = Instant::now();

    client.write_all(&payload).await.expect("write failed");
    let mut resp = vec![0u8; payload.len()];
    client.read_exact(&mut resp).await.expect("read failed");
    let elapsed = start.elapsed();

    // 10KB at 10KB/s takes ~1.0s in each direction
    assert!(
        elapsed >= Duration::from_millis(900),
        "Expected >= 900ms duration for throttled transfer, got {:?}",
        elapsed
    );

    proxy.shutdown();
}

#[tokio::test]
async fn test_close_after_bytes_severing() {
    let echo_addr = spawn_echo_server().await;

    let config = Config {
        listen: "127.0.0.1:0".parse().unwrap(),
        target: echo_addr,
        format: OutputFormat::None,
        delay_ms: 0,
        jitter_ms: 0,
        rate_limit_bytes_per_sec: None,
        corrupt_rate: 0.0,
        drop_rate: 0.0,
        close_after_bytes: Some(30),
        close_after_secs: None,
        reset: false,
        quiet: true,
    };

    let proxy = start_proxy(config).await.expect("Failed to start tcptap");
    let proxy_addr = proxy.local_addr();

    let mut client = TcpStream::connect(proxy_addr).await.expect("connect failed");

    // Write 50 bytes
    let payload = vec![0x7f; 50];
    let _ = client.write_all(&payload).await;

    // Reading should hit EOF (connection closed by proxy)
    let mut buf = vec![0u8; 100];
    let mut total_read = 0;
    while let Ok(n) = client.read(&mut buf).await {
        if n == 0 {
            break;
        }
        total_read += n;
    }

    assert!(total_read <= 30, "Should have been severed after 30 bytes");
    proxy.shutdown();
}
