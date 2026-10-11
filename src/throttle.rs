use anyhow::{bail, Result};
use std::time::{Duration, Instant};

/// Parse a human-friendly rate limit string like "100KB/s", "500B/s", "10MB/s", "50k" into bytes/sec
pub fn parse_rate_limit(s: &str) -> Result<u64> {
    let clean = s.trim().to_lowercase();
    let clean = clean.trim_end_matches("/s").trim_end_matches("ps");

    if let Some(num_str) = clean.strip_suffix("gb").or_else(|| clean.strip_suffix('g')) {
        let n: f64 = num_str.trim().parse()?;
        return Ok((n * 1024.0 * 1024.0 * 1024.0) as u64);
    }
    if let Some(num_str) = clean.strip_suffix("mb").or_else(|| clean.strip_suffix('m')) {
        let n: f64 = num_str.trim().parse()?;
        return Ok((n * 1024.0 * 1024.0) as u64);
    }
    if let Some(num_str) = clean.strip_suffix("kb").or_else(|| clean.strip_suffix('k')) {
        let n: f64 = num_str.trim().parse()?;
        return Ok((n * 1024.0) as u64);
    }
    if let Some(num_str) = clean.strip_suffix('b') {
        let n: f64 = num_str.trim().parse()?;
        return Ok(n as u64);
    }

    if let Ok(n) = clean.parse::<u64>() {
        return Ok(n);
    }

    bail!("Invalid rate limit format '{}', expected e.g. '100KB/s', '1MB/s', '500B/s'", s)
}

pub struct RateLimiter {
    bytes_per_sec: f64,
    start_time: Instant,
    total_bytes: u64,
}

impl RateLimiter {
    pub fn new(bytes_per_sec: u64) -> Self {
        Self {
            bytes_per_sec: bytes_per_sec as f64,
            start_time: Instant::now(),
            total_bytes: 0,
        }
    }

    pub async fn throttle(&mut self, chunk_len: usize) {
        if self.bytes_per_sec <= 0.0 || chunk_len == 0 {
            return;
        }

        self.total_bytes += chunk_len as u64;
        let expected_duration_secs = self.total_bytes as f64 / self.bytes_per_sec;
        let elapsed_secs = self.start_time.elapsed().as_secs_f64();

        if expected_duration_secs > elapsed_secs {
            let sleep_secs = expected_duration_secs - elapsed_secs;
            tokio::time::sleep(Duration::from_secs_f64(sleep_secs)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_rate_limit() {
        assert_eq!(parse_rate_limit("500B/s").unwrap(), 500);
        assert_eq!(parse_rate_limit("100KB/s").unwrap(), 100 * 1024);
        assert_eq!(parse_rate_limit("50k").unwrap(), 50 * 1024);
        assert_eq!(parse_rate_limit("10MB/s").unwrap(), 10 * 1024 * 1024);
        assert_eq!(parse_rate_limit("2m").unwrap(), 2 * 1024 * 1024);
        assert_eq!(parse_rate_limit("10000").unwrap(), 10000);
    }
}
