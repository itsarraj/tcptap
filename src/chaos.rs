use rand::Rng;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ChaosConfig {
    pub delay_ms: u64,
    pub jitter_ms: u64,
    pub corrupt_rate: f64,
    pub drop_rate: f64,
}

impl ChaosConfig {
    pub async fn apply_delay(&self) {
        if self.delay_ms == 0 && self.jitter_ms == 0 {
            return;
        }

        let base = self.delay_ms as i64;
        let jitter = if self.jitter_ms > 0 {
            let mut rng = rand::thread_rng();
            rng.gen_range(-(self.jitter_ms as i64)..=(self.jitter_ms as i64))
        } else {
            0
        };

        let total_ms = (base + jitter).max(0) as u64;
        if total_ms > 0 {
            tokio::time::sleep(Duration::from_millis(total_ms)).await;
        }
    }

    pub fn should_drop(&self) -> bool {
        if self.drop_rate <= 0.0 {
            false
        } else if self.drop_rate >= 1.0 {
            true
        } else {
            rand::thread_rng().gen::<f64>() < self.drop_rate
        }
    }

    pub fn maybe_corrupt(&self, data: &mut [u8]) -> bool {
        if self.corrupt_rate <= 0.0 || data.is_empty() {
            return false;
        }

        let mut rng = rand::thread_rng();
        if rng.gen::<f64>() < self.corrupt_rate {
            // Corrupt a random byte
            let idx = rng.gen_range(0..data.len());
            data[idx] ^= rng.gen_range(1..=255);
            true
        } else {
            false
        }
    }
}
