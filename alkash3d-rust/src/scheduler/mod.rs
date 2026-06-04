//! Планировщик задач для 4-8 ядер

mod budget;
mod pool;
mod task;
mod adaptive;

pub use budget::*;
pub use pool::*;
pub use task::*;
pub use adaptive::*;

use std::sync::Arc;
use std::time::Duration;

/// Основной планировщик движка
pub struct EngineScheduler {
    pub cpu_budget: Arc<CpuBudget>,
    pub worker_pool: Arc<WorkerPool>,
    stats: SchedulerStats,
    thresholds: AdaptiveThresholds,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SchedulerStats {
    pub frames: u32,
    pub avg_frame_time_ms: f32,
    pub max_frame_time_ms: f32,
    pub tasks_heavy: u32,
    pub tasks_light: u32,
    pub cpu_usage_percent: f32,
    pub time_broad_phase_ms: f32,
    pub time_narrow_phase_ms: f32,
    pub time_solver_ms: f32,
    pub time_render_ms: f32,
    pub time_culling_ms: f32,
}

impl EngineScheduler {
    pub fn new() -> Self {
        let total_cores = num_cpus::get();
        let heavy_cores = (total_cores / 2).max(2).min(4);
        let light_cores = total_cores;

        Self {
            cpu_budget: Arc::new(CpuBudget::with_reserve(1)),
            worker_pool: Arc::new(WorkerPool::new(heavy_cores, light_cores)),
            stats: SchedulerStats::default(),
            thresholds: AdaptiveThresholds::new(ThresholdConfig::default()),
        }
    }

    pub fn execute<F>(&self, task: Task, f: F) -> bool
    where
        F: FnOnce() + Send + 'static,
    {
        let needed = task.priority.cores();
        let budget = self.cpu_budget.clone();

        if !budget.try_acquire(needed) {
            return false;
        }

        self.worker_pool.spawn(task, move || {
            f();
            budget.release(needed);
        });
        true
    }

    #[inline]
    pub fn broad_phase_threshold(&self) -> usize {
        self.thresholds.broad_phase_threshold()
    }

    #[inline]
    pub fn narrow_phase_threshold(&self) -> usize {
        self.thresholds.narrow_phase_threshold()
    }

    pub fn update_stats(&mut self, frame_time: Duration, timings: FrameTimings) {
        let alpha = 0.9;
        self.stats.avg_frame_time_ms = self.stats.avg_frame_time_ms * alpha
            + (frame_time.as_secs_f32() * 1000.0) * (1.0 - alpha);
        self.stats.max_frame_time_ms = self.stats.max_frame_time_ms
            .max(frame_time.as_secs_f32() * 1000.0);
        self.stats.time_broad_phase_ms = timings.broad_phase_ms;
        self.stats.time_narrow_phase_ms = timings.narrow_phase_ms;
        self.stats.time_solver_ms = timings.solver_ms;
        self.stats.time_render_ms = timings.render_ms;
        self.stats.time_culling_ms = timings.culling_ms;

        let total = num_cpus::get() as f32;
        let used = total - self.cpu_budget.available_cores() as f32;
        self.stats.cpu_usage_percent = (used / total) * 100.0;

        self.thresholds.adapt(self.stats.avg_frame_time_ms, 16.6);
        self.stats.frames += 1;
    }

    pub fn stats(&self) -> &SchedulerStats {
        &self.stats
    }

    pub fn reset_budget(&self) {
        self.cpu_budget.reset_frame();
    }
}

impl Default for EngineScheduler {
    fn default() -> Self {
        Self::new()
    }
}