use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Instant, Duration};

pub struct AssetCache<T: Clone + Send + Sync> {
    data: RwLock<HashMap<String, CacheEntry<T>>>,
    ttl: Duration,
    max_size: usize,
}

struct CacheEntry<T> {
    value: T,
    created: Instant,
}

impl<T: Clone + Send + Sync> AssetCache<T> {
    pub fn new(ttl_seconds: f32, max_size: usize) -> Self {
        Self {
            data: RwLock::new(HashMap::new()),
            ttl: Duration::from_secs_f32(ttl_seconds),
            max_size,
        }
    }

    pub fn get(&self, key: &str) -> Option<T> {
        if let Ok(data) = self.data.read() {
            if let Some(entry) = data.get(key) {
                if entry.created.elapsed() < self.ttl {
                    return Some(entry.value.clone());
                }
            }
        }
        None
    }

    pub fn insert(&self, key: String, value: T) {
        if let Ok(mut data) = self.data.write() {
            if data.len() >= self.max_size {
                let oldest = data.iter()
                    .min_by_key(|(_, e)| e.created)
                    .map(|(k, _)| k.clone());
                if let Some(key) = oldest {
                    data.remove(&key);
                }
            }
            data.insert(key, CacheEntry { value, created: Instant::now() });
        }
    }

    pub fn cleanup(&self) {
        if let Ok(mut data) = self.data.write() {
            let now = Instant::now();
            data.retain(|_, e| now.duration_since(e.created) < self.ttl);
        }
    }
}