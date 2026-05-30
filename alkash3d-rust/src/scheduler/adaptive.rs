//! Адаптивный порог параллелизации

use std::sync::atomic::{AtomicUsize, Ordering};

/// Динамические пороги параллелизации
pub struct AdaptiveThresholds {
    /// Базовая конфигурация
    base: ThresholdConfig,
    /// Текущие множители
    multipliers: AtomicUsize,
}

#[derive(Debug, Clone, Copy)]
pub struct ThresholdConfig {
    pub broad_phase_bodies: usize,
    pub narrow_phase_pairs: usize,
    pub solver_contacts: usize,
    pub culling_lights: usize,
}

impl Default for ThresholdConfig {
    fn default() -> Self {
        Self {
            broad_phase_bodies: 50,
            narrow_phase_pairs: 500,
            solver_contacts: 1000,
            culling_lights: 200,
        }
    }
}

impl AdaptiveThresholds {
    pub fn new(base: ThresholdConfig) -> Self {
        Self {
            base,
            multipliers: AtomicUsize::new(1),
        }
    }

    /// Адаптация под текущую нагрузку
    pub fn adapt(&self, frame_time_ms: f32, target_ms: f32) {
        let ratio = frame_time_ms / target_ms;

        let new_mult = if ratio > 1.2 {
            2
        } else if ratio < 0.7 {
            1
        } else {
            1
        };

        self.multipliers.store(new_mult, Ordering::Relaxed);
    }

    #[inline]
    pub fn broad_phase_threshold(&self) -> usize {
        self.base.broad_phase_bodies * self.multipliers.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn narrow_phase_threshold(&self) -> usize {
        self.base.narrow_phase_pairs * self.multipliers.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn solver_threshold(&self) -> usize {
        self.base.solver_contacts * self.multipliers.load(Ordering::Relaxed)
    }

    #[inline]
    pub fn culling_threshold(&self) -> usize {
        self.base.culling_lights
    }
}