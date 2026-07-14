// Copyright 2025 N0, INC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::time::{Duration, Instant};

use byte_unit::{Bit, UnitType};
use iroh::endpoint::ConnectionStats;

/// Spawn a named OS thread and panic if spawning fails.
pub fn spawn_thread<F, T>(name: impl ToString, f: F) -> std::thread::JoinHandle<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let name_str = name.to_string();
    std::thread::Builder::new()
        .name(name_str.clone())
        .spawn(f)
        .unwrap_or_else(|_| panic!("failed to spawn thread: {}", name_str))
}

pub struct StatsSmoother {
    rate_up: Rate,
    rate_down: Rate,
    last_update: Instant,
    rtt: Duration,
}

impl Default for StatsSmoother {
    fn default() -> Self {
        Self::new()
    }
}

impl StatsSmoother {
    pub fn new() -> Self {
        Self {
            rate_up: Default::default(),
            rate_down: Default::default(),
            last_update: Instant::now(),
            rtt: Duration::from_secs(0),
        }
    }
    /// iroh 1.0's `PathList` borrows the connection (has a lifetime), so it can
    /// no longer be returned by value from `total`. The caller now precomputes
    /// the selected-path RTT (e.g. `conn.paths().iter().find(|p|
    /// p.is_selected()).map(|p| p.rtt())`) and hands it in directly.
    pub fn smoothed(
        &mut self,
        total: impl FnOnce() -> (ConnectionStats, Duration),
    ) -> SmoothedStats<'_> {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_update);
        if elapsed >= Duration::from_secs(1) {
            let (stats, rtt) = (total)();
            self.rate_down.update(elapsed, stats.udp_rx.bytes);
            self.rate_up.update(elapsed, stats.udp_tx.bytes);
            self.last_update = now;
            self.rtt = rtt;
        }
        SmoothedStats {
            down: &self.rate_down,
            up: &self.rate_up,
            rtt: self.rtt,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Rate {
    /// Total bytes
    pub total: u64,
    /// Rate in bytes per second
    pub rate: f32,
    /// Rate rendered as a string
    pub rate_str: String,
}

impl Rate {
    fn update(&mut self, delta_time: Duration, new_total: u64) {
        let delta = new_total.saturating_sub(self.total);
        let delta_secs = delta_time.as_secs_f32();
        let rate = if delta_secs > 0.0 && delta > 0 {
            (delta as f32 * 8.0) / delta_secs
        } else {
            0.0
        };
        let bit = Bit::from_f32(rate).unwrap();
        let adjusted = bit.get_appropriate_unit(UnitType::Decimal);
        self.rate = rate;
        self.rate_str = format!("{adjusted:.2}/s");
        self.total = new_total;
    }
}

pub struct SmoothedStats<'a> {
    pub rtt: Duration,
    pub down: &'a Rate,
    pub up: &'a Rate,
}
