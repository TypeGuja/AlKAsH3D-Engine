// src/config.rs
pub struct PerformanceConfig {
    // Память
    pub frame_allocator_size_mb: usize,     // 256 MB
    pub asset_cache_size: usize,            // 1024 записи
    pub object_pool_size: usize,            // 100000 объектов
    pub gpu_buffer_pool_size: usize,        // 5000 буферов

    // Рендеринг
    pub enable_instancing: bool,            // true
    pub batch_size: usize,                  // 256 объектов на батч
    pub max_draw_calls: u32,                // 10000
    pub lod_distances: [f32; 4],           // [10.0, 50.0, 100.0, 500.0]

    // Оптимизации
    pub use_frustum_culling: bool,          // true
    pub use_occlusion_culling: bool,        // false (пока)
    pub parallel_processing: bool,          // true
    pub async_loading: bool,                // true

    // GPU
    pub vsync: bool,                        // false
    pub target_fps: u32,                    // 144
    pub max_gpu_latency_frames: u32,        // 2
}

impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            frame_allocator_size_mb: 256,
            asset_cache_size: 1024,
            object_pool_size: 100000,
            gpu_buffer_pool_size: 5000,
            enable_instancing: true,
            batch_size: 256,
            max_draw_calls: 10000,
            lod_distances: [10.0, 50.0, 100.0, 500.0],
            use_frustum_culling: true,
            use_occlusion_culling: false,
            parallel_processing: true,
            async_loading: true,
            vsync: false,
            target_fps: 144,
            max_gpu_latency_frames: 2,
        }
    }
}