// src/memory/allocator.rs

use std::alloc::{Layout, alloc};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct FrameAllocator {
    buffer: *mut u8,
    size: usize,
    used: AtomicUsize,
}

unsafe impl Send for FrameAllocator {}
unsafe impl Sync for FrameAllocator {}

impl FrameAllocator {
    pub fn new(size_mb: usize) -> Self {
        let size = size_mb * 1024 * 1024;
        let layout = Layout::from_size_align(size, 64).unwrap();
        let buffer = unsafe { alloc(layout) };
        Self {
            buffer,
            size,
            used: AtomicUsize::new(0),
        }
    }

    pub fn allocate<T>(&self, count: usize) -> &mut [T] {
        let byte_size = count * std::mem::size_of::<T>();
        let offset = self.used.fetch_add(byte_size, Ordering::Relaxed);
        if offset + byte_size <= self.size {
            unsafe {
                std::slice::from_raw_parts_mut(
                    self.buffer.add(offset) as *mut T,
                    count,
                )
            }
        } else {
            panic!("Frame allocator out of memory");
        }
    }

    pub fn reset_frame(&self) {
        self.used.store(0, Ordering::Relaxed);
    }

    // ДОБАВЬТЕ ЭТОТ МЕТОД
    pub fn usage_percent(&self) -> f64 {
        let used = self.used.load(Ordering::Relaxed);
        if self.size == 0 {
            return 0.0;
        }
        (used as f64 / self.size as f64) * 100.0
    }
}

impl Drop for FrameAllocator {
    fn drop(&mut self) {
        if !self.buffer.is_null() {
            let layout = Layout::from_size_align(self.size, 64).unwrap();
            unsafe { std::alloc::dealloc(self.buffer, layout) };
        }
    }
}