//! Бюджет ядер процессора

use std::sync::atomic::{AtomicUsize, Ordering};

pub struct CpuBudget {
    available: AtomicUsize,
    reserve: usize,
    total_cores: usize,
}

impl CpuBudget {
    pub fn new() -> Self {
        Self::with_reserve(1)
    }

    pub fn with_reserve(reserve: usize) -> Self {
        let total_cores = num_cpus::get();
        let available = total_cores.saturating_sub(reserve);

        Self {
            available: AtomicUsize::new(available),
            reserve,
            total_cores,
        }
    }

    pub fn try_acquire(&self, needed: usize) -> bool {
        let mut current = self.available.load(Ordering::Acquire);

        loop {
            if current < needed {
                return false;
            }

            match self.available.compare_exchange_weak(
                current,
                current - needed,
                Ordering::Release,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(new) => current = new,
            }
        }
    }

    pub fn release(&self, cores: usize) {
        self.available.fetch_add(cores, Ordering::Release);
    }

    pub fn available_cores(&self) -> usize {
        self.available.load(Ordering::Relaxed)
    }

    pub fn reset_frame(&self) {
        self.available.store(
            self.total_cores - self.reserve,
            Ordering::Release,
        );
    }
}

impl Default for CpuBudget {
    fn default() -> Self {
        Self::new()
    }
}