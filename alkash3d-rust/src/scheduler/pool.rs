//! Пул рабочих потоков

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{self, JoinHandle};
use crate::scheduler::task::{Task, TaskPriority};

pub struct WorkerPool {
    heavy_workers: Vec<Worker>,
    light_workers: Vec<Worker>,
}

pub struct Worker {
    handle: JoinHandle<()>,
    sender: Sender<Box<dyn FnOnce() + Send + 'static>>,
}

impl Worker {
    fn new(name: &str, priority: ThreadPriority) -> Self {
        let (tx, rx): (
            Sender<Box<dyn FnOnce() + Send + 'static>>,
            Receiver<Box<dyn FnOnce() + Send + 'static>>,
        ) = channel();

        let handle = thread::Builder::new()
            .name(name.to_string())
            .spawn(move || {
                #[cfg(target_os = "windows")]
                unsafe {
                    use windows::Win32::System::Threading::*;
                    let priority_class = match priority {
                        ThreadPriority::High => THREAD_PRIORITY_HIGHEST,
                        ThreadPriority::Normal => THREAD_PRIORITY_NORMAL,
                        ThreadPriority::Low => THREAD_PRIORITY_LOWEST,
                    };
                    SetThreadPriority(GetCurrentThread(), priority_class);
                }

                for f in rx {
                    f();
                }
            })
            .unwrap();

        Self { handle, sender: tx }
    }

    fn send(&self, f: Box<dyn FnOnce() + Send + 'static>) {
        let _ = self.sender.send(f);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadPriority {
    High,
    Normal,
    Low,
}

impl WorkerPool {
    pub fn new(heavy_count: usize, light_count: usize) -> Self {
        let mut heavy_workers = Vec::with_capacity(heavy_count);
        let mut light_workers = Vec::with_capacity(light_count);

        for i in 0..heavy_count {
            heavy_workers.push(Worker::new(
                &format!("heavy-{}", i),
                ThreadPriority::High,
            ));
        }

        for i in 0..light_count {
            light_workers.push(Worker::new(
                &format!("light-{}", i),
                ThreadPriority::Normal,
            ));
        }

        Self {
            heavy_workers,
            light_workers,
        }
    }

    pub fn spawn<F>(&self, task: Task, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let workers = match task.priority {
            TaskPriority::High | TaskPriority::Critical => &self.heavy_workers,
            TaskPriority::Normal | TaskPriority::Low => &self.light_workers,
        };

        if workers.is_empty() {
            f();
            return;
        }

        use std::cell::RefCell;
        thread_local! {
            static LAST_IDX: RefCell<usize> = RefCell::new(0);
        }

        let idx = LAST_IDX.with(|cell| {
            let mut val = cell.borrow_mut();
            let idx = *val % workers.len();
            *val = val.wrapping_add(1);
            idx
        });

        workers[idx].send(Box::new(f));
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        for worker in &self.heavy_workers {
            drop(&worker.sender);
        }
        for worker in &self.light_workers {
            drop(&worker.sender);
        }
        for worker in self.heavy_workers.drain(..) {
            let _ = worker.handle.join();
        }
        for worker in self.light_workers.drain(..) {
            let _ = worker.handle.join();
        }
    }
}