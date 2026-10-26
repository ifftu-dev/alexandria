//! IPC command rate limiter for sensitive operations.
//!
//! Uses a per-command token bucket to limit how frequently sensitive
//! commands (export_mnemonic, mint_nft, etc.) can be invoked.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Per-command rate limit configuration.
struct Bucket {
    /// Maximum tokens (burst capacity).
    max_tokens: u32,
    /// Current token count.
    tokens: u32,
    /// Token refill interval.
    refill_interval: Duration,
    /// Last refill time.
    last_refill: Instant,
}

impl Bucket {
    fn new(max_tokens: u32, refill_interval: Duration) -> Self {
        Self {
            max_tokens,
            tokens: max_tokens,
            refill_interval,
            last_refill: Instant::now(),
        }
    }

    fn try_consume(&mut self) -> bool {
        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill);
        let refills = (elapsed.as_millis() / self.refill_interval.as_millis().max(1)) as u32;
        if refills > 0 {
            self.tokens = (self.tokens + refills).min(self.max_tokens);
            self.last_refill = now;
        }

        if self.tokens > 0 {
            self.tokens -= 1;
            true
        } else {
            false
        }
    }
}

/// Rate limiter for Tauri IPC commands.
pub struct IpcRateLimiter {
    buckets: HashMap<String, Bucket>,
}

impl IpcRateLimiter {
    /// Create a new rate limiter with default limits for sensitive commands.
    pub fn new() -> Self {
        let mut buckets = HashMap::new();

        // export_mnemonic: 3 attempts per 5 minutes
        buckets.insert(
            "export_mnemonic".to_string(),
            Bucket::new(3, Duration::from_secs(100)), // 1 refill per 100s → 3 per 5min
        );

        // Plugin install: a slow, security-sensitive operation (manifest
        // parse, signature verify, disk copy). 10 per 10 min is generous
        // for interactive use and low enough that a script-driven loop
        // can't wedge the app.
        buckets.insert(
            "plugin_install_from_file".to_string(),
            Bucket::new(10, Duration::from_secs(60)),
        );
        // Plugin grade: each call compiles+runs WASM and writes a
        // submission row. 60 per minute is generous for an interactive
        // assessment and caps a runaway plugin that hammers the grader.
        buckets.insert(
            "plugin_submit_and_grade".to_string(),
            Bucket::new(60, Duration::from_secs(6)),
        );

        Self { buckets }
    }

    /// Check if a command is allowed. Returns `Ok(())` if under limit,
    /// `Err(message)` if rate-limited.
    pub fn check(&mut self, command: &str) -> Result<(), String> {
        if let Some(bucket) = self.buckets.get_mut(command) {
            if bucket.try_consume() {
                Ok(())
            } else {
                Err(format!(
                    "rate limited: too many calls to '{command}', please wait"
                ))
            }
        } else {
            // No rate limit configured for this command
            Ok(())
        }
    }
}

impl Default for IpcRateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_within_limit() {
        let mut limiter = IpcRateLimiter::new();
        assert!(limiter.check("export_mnemonic").is_ok());
        assert!(limiter.check("export_mnemonic").is_ok());
        assert!(limiter.check("export_mnemonic").is_ok());
    }

    #[test]
    fn blocks_over_limit() {
        let mut limiter = IpcRateLimiter::new();
        assert!(limiter.check("export_mnemonic").is_ok());
        assert!(limiter.check("export_mnemonic").is_ok());
        assert!(limiter.check("export_mnemonic").is_ok());
        // 4th call should be blocked
        assert!(limiter.check("export_mnemonic").is_err());
    }

    #[test]
    fn unconfigured_command_always_allowed() {
        let mut limiter = IpcRateLimiter::new();
        for _ in 0..100 {
            assert!(limiter.check("some_random_command").is_ok());
        }
    }
}
