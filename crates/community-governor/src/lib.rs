//! Resource Governor & User Experience Preservation (UEPS) engine.
//! Ensures user tasks always take absolute priority over community AI compute.

use serde::{Deserialize, Serialize};
use sysinfo::{CpuRefreshKind, MemoryRefreshKind, RefreshKind, System};
use std::time::{Duration, Instant};
use community_protocol::{ThermalState, UserActivity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GovernorState {
    Idle,
    Available,
    Contributing,
    Throttling,
    Paused,
    Resuming,
    Offline,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernorConfig {
    pub max_cpu_usage_pct: f32,
    pub min_free_ram_mb: usize,
    pub pause_on_battery: bool,
    pub target_ueps: f32,
}

impl Default for GovernorConfig {
    fn default() -> Self {
        Self {
            max_cpu_usage_pct: 75.0,
            min_free_ram_mb: 1024,
            pause_on_battery: true,
            target_ueps: 0.95,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernorMetrics {
    pub state: GovernorState,
    pub capacity: f32,
    pub ueps: f32,
    pub cpu_usage_pct: f32,
    pub available_memory_mb: usize,
    pub user_activity: UserActivity,
    pub thermal_state: ThermalState,
    pub on_battery: bool,
}

pub struct ResourceGovernor {
    config: GovernorConfig,
    system: System,
    current_state: GovernorState,
    current_capacity: f32,
    last_user_activity: Instant,
}

impl ResourceGovernor {
    pub fn new(config: GovernorConfig) -> Self {
        let system = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        Self {
            config,
            system,
            current_state: GovernorState::Available,
            current_capacity: 1.0,
            last_user_activity: Instant::now(),
        }
    }

    /// Refreshes host hardware metrics and updates the governor state & capacity.
    pub fn tick(&mut self, is_busy_owner: bool, on_battery: bool) -> GovernorMetrics {
        self.system.refresh_cpu_all();
        self.system.refresh_memory();

        let cpu_usage = self.system.global_cpu_usage();
        let _total_mem = (self.system.total_memory() / 1024 / 1024) as usize;
        let available_mem = (self.system.available_memory() / 1024 / 1024) as usize;

        if is_busy_owner {
            self.last_user_activity = Instant::now();
        }

        let idle_duration = self.last_user_activity.elapsed();
        let user_activity = if idle_duration < Duration::from_secs(5) || is_busy_owner {
            UserActivity::Busy
        } else if idle_duration < Duration::from_secs(30) {
            UserActivity::Active
        } else if idle_duration < Duration::from_secs(120) {
            UserActivity::Light
        } else {
            UserActivity::Idle
        };

        // Compute User Experience Preservation Score (UEPS)
        let cpu_contention = (cpu_usage / 100.0).clamp(0.0, 1.0);
        let mem_contention = if available_mem < self.config.min_free_ram_mb { 0.8 } else { 0.0 };
        let ueps = (1.0 - (0.5 * cpu_contention + 0.5 * mem_contention)).clamp(0.0, 1.0);

        // State Machine & Capacity derivation
        let (state, capacity) = if on_battery && self.config.pause_on_battery {
            (GovernorState::Paused, 0.0)
        } else if available_mem < self.config.min_free_ram_mb {
            (GovernorState::Paused, 0.0)
        } else {
            match user_activity {
                UserActivity::Busy => (GovernorState::Paused, 0.0),
                UserActivity::Active => (GovernorState::Throttling, 0.2),
                UserActivity::Light => (GovernorState::Contributing, 0.5),
                UserActivity::Idle => (GovernorState::Contributing, 0.85),
            }
        };

        self.current_state = state;
        self.current_capacity = capacity;

        GovernorMetrics {
            state,
            capacity,
            ueps,
            cpu_usage_pct: cpu_usage,
            available_memory_mb: available_mem,
            user_activity,
            thermal_state: ThermalState::Normal,
            on_battery,
        }
    }

    pub fn current_capacity(&self) -> f32 {
        self.current_capacity
    }

    pub fn current_state(&self) -> GovernorState {
        self.current_state
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_governor_battery_pause() {
        let mut gov = ResourceGovernor::new(GovernorConfig::default());
        let m = gov.tick(false, true);
        assert_eq!(m.state, GovernorState::Paused);
        assert_eq!(m.capacity, 0.0);
    }

    #[test]
    fn test_governor_user_busy_pause() {
        let mut gov = ResourceGovernor::new(GovernorConfig::default());
        let m = gov.tick(true, false);
        assert_eq!(m.state, GovernorState::Paused);
        assert_eq!(m.capacity, 0.0);
    }
}
