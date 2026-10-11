# tcptap

A zero-daemon TCP tap and chaos proxy with duplex inspection, latency injection, bandwidth throttling, and socket fault injection.

`tcptap` acts as an in-line bidirectional proxy between any TCP client and upstream server (PostgreSQL, Redis, MySQL, HTTP/1.1, gRPC, custom binary protocols). Unlike tools like `Toxiproxy` that require running separate background daemons and configuring them via HTTP APIs, `tcptap` runs directly from your command line on-the-fly.

## Features

- **Bi-Directional Traffic Inspection**:
  - Live color-coded inspection: client traffic (`>>>`) in cyan, upstream traffic (`<<<`) in green.
  - Three output formats:
    - `hex`: Canonical 16-byte width hexdump with byte offsets, hex pairs, and ASCII representation.
    - `text`: Printable ASCII/UTF-8 with non-printables escaped (`\xHH`).
    - `none`: Silent pass-through for high-throughput benchmarking.
- **Bandwidth Throttling / Rate-Limiting**:
  - Token-bucket byte pacing with `--rate-limit` (e.g. `50KB/s`, `1MB/s`, `500B/s`).
  - Accurately simulates slow 2G/3G mobile networks, bandwidth-constrained IoT links, or high-latency satellite connections.
- **Latency & Jitter Injection**:
  - Add fixed latency with `--delay-ms <MS>`.
  - Add uniform random jitter with `--jitter-ms <MS>`.
- **Packet Corruption & Dropping**:
  - Corrupt random payload bytes with probability `--corrupt-rate <0.0-1.0>`.
  - Drop entire chunk frames with probability `--drop-rate <0.0-1.0>`.
- **Connection Severing & Fault Injection**:
  - Abruptly sever socket connections after transferring $N$ bytes (`--close-after-bytes <N>`).
  - Abruptly sever socket connections after $T$ seconds (`--close-after-secs <SECS>`).
  - Supports `--reset` to inject TCP RST instead of graceful FIN.

## Installation

```bash
cargo install --path .
```

## Quick Start

### 1. Inspect PostgreSQL or Redis Traffic in Hex

```bash
# Listen on 5433 and forward to local PostgreSQL on 5432
tcptap --listen 127.0.0.1:5433 --target 127.0.0.1:5432 --format hex
```

### 2. Simulate High Latency and Jitter

```bash
# Add 100ms base latency with +/- 25ms random jitter
tcptap --listen 127.0.0.1:9000 --target 127.0.0.1:6379 --delay-ms 100 --jitter-ms 25
```

### 3. Throttle Bandwidth to Test Slow Network Recovery

```bash
# Limit data transfer rate to 50 KB/s
tcptap --listen 127.0.0.1:8000 --target 127.0.0.1:8080 --rate-limit 50KB/s
```

### 4. Chaos Test: Packet Drop & Abrupt Socket Severing

```bash
# Drop 2% of chunks and sever connection after 100KB transferred
tcptap --listen 127.0.0.1:7000 --target 127.0.0.1:7001 --drop-rate 0.02 --close-after-bytes 102400
```

## Command-Line Options

```text
Usage: tcptap [OPTIONS] --target <TARGET>

Options:
  -l, --listen <LISTEN>                  Socket address to listen on [default: 127.0.0.1:9000]
  -t, --target <TARGET>                  Target upstream socket address to forward traffic to
  -f, --format <FORMAT>                  Output inspection format: 'hex', 'text', 'none' [default: hex]
      --delay-ms <MS>                    Artificial latency delay in milliseconds [default: 0]
      --jitter-ms <MS>                   Random latency jitter in milliseconds [default: 0]
      --rate-limit <LIMIT>               Bandwidth throttling rate limit (e.g. '100KB/s', '1MB/s')
      --corrupt-rate <RATE>              Packet corruption probability rate (0.0 to 1.0) [default: 0.0]
      --drop-rate <RATE>                 Packet drop probability rate (0.0 to 1.0) [default: 0.0]
      --close-after-bytes <BYTES>        Sever connection after transferring N total bytes
      --close-after-secs <SECS>          Sever connection after N seconds
      --reset                            Sever connections abruptly with TCP RST
  -q, --quiet                            Suppress connection status messages
  -h, --help                             Print help
  -V, --version                          Print version
```

## Status

Fully functional and verified. Duplex traffic inspection (hex, text, raw), rate-limiting bandwidth throttling, artificial latency and jitter, packet corruption, packet dropping, and connection severing are implemented and verified by automated unit and live socket integration test suites.
