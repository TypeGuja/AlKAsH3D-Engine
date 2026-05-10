use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

pub struct ObjectPool<T> {
    available: Arc<Mutex<VecDeque<T>>>,
    factory: fn() -> T,
    max_size: usize,
    created_count: usize,
}

impl<T: Send + 'static> ObjectPool<T> {
    pub fn new(factory: fn() -> T, initial_capacity: usize, max_size: usize) -> Self {
        let mut available = VecDeque::with_capacity(initial_capacity);
        for _ in 0..initial_capacity {
            available.push_back(factory());
        }
        Self {
            available: Arc::new(Mutex::new(available)),
            factory,
            max_size,
            created_count: initial_capacity,
        }
    }

    pub fn acquire(&mut self) -> T {
        if let Ok(mut available) = self.available.lock() {
            if let Some(obj) = available.pop_front() {
                return obj;
            }
        }
        if self.created_count < self.max_size {
            self.created_count += 1;
            return (self.factory)();
        }
        loop {
            if let Ok(mut available) = self.available.lock() {
                if let Some(obj) = available.pop_front() {
                    return obj;
                }
            }
            std::hint::spin_loop();
        }
    }

    pub fn release(&self, obj: T) {
        if let Ok(mut available) = self.available.lock() {
            if available.len() < self.max_size {
                available.push_back(obj);
            }
        }
    }
}