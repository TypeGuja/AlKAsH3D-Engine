use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use crate::math::Vec3;
use crate::math::Quat;

#[derive(Debug, Clone)]
pub struct WorldChunk {
    pub grid_pos: (i32, i32, i32),
    pub objects: Vec<ChunkObject>,
    pub state: ChunkLoadState,
    pub priority: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChunkLoadState {
    Unloaded,
    Loading,
    Loaded,
    Unloading,
}

#[derive(Debug, Clone)]
pub struct ChunkObject {
    pub name: String,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    pub mesh_id: String,
}

#[derive(Debug, Clone)]
pub struct WorldStreamer {
    pub active_world: Option<String>,
    pub chunks: HashMap<(i32, i32, i32), WorldChunk>,
    pub loading_queue: VecDeque<(i32, i32, i32)>,
    pub max_concurrent_loads: usize,
    pub load_distance: f32,
    pub unload_distance: f32,
    pub chunk_size: f32,
    stream_thread_active: Arc<AtomicBool>,
}

impl WorldStreamer {
    pub fn new() -> Self {
        Self {
            active_world: None,
            chunks: HashMap::new(),
            loading_queue: VecDeque::new(),
            max_concurrent_loads: 3,
            load_distance: 200.0,
            unload_distance: 250.0,
            chunk_size: 64.0,
            stream_thread_active: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn load_world(&mut self, path: &str) -> Result<(), String> {
        self.active_world = Some(path.to_string());
        self.chunks.clear();

        for x in -4..4 {
            for z in -4..4 {
                self.chunks.insert((x, 0, z), WorldChunk {
                    grid_pos: (x, 0, z),
                    objects: Vec::new(),
                    state: ChunkLoadState::Unloaded,
                    priority: 0.0,
                });
            }
        }

        Ok(())
    }

    pub fn update_streaming(&mut self, player_pos: Vec3) {
        for ((x, y, z), chunk) in &mut self.chunks {
            let center = Vec3::new(
                *x as f32 * self.chunk_size + self.chunk_size * 0.5,
                *y as f32 * self.chunk_size,
                *z as f32 * self.chunk_size + self.chunk_size * 0.5,
            );
            let dist = center.distance(player_pos);

            if dist < self.load_distance && chunk.state == ChunkLoadState::Unloaded {
                chunk.state = ChunkLoadState::Loading;
                self.loading_queue.push_back((*x, *y, *z));
            } else if dist > self.unload_distance && chunk.state == ChunkLoadState::Loaded {
                chunk.state = ChunkLoadState::Unloading;
            }
        }
    }

    pub fn process_loading_queue(&mut self) {
        while let Some(pos) = self.loading_queue.pop_front() {
            if let Some(chunk) = self.chunks.get_mut(&pos) {
                chunk.state = ChunkLoadState::Loaded;
            }
        }
    }
}