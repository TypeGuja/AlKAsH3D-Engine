use nalgebra::Vector3;

#[derive(Debug, Clone)]
pub struct LightGrid {
    pub cells: Vec<crate::LightGridCell>,
    pub entries: Vec<crate::LightGridEntry>,
    pub grid_width: u32,
    pub grid_height: u32,
    pub grid_depth: u32,
    pub cell_size: f32,
    pub world_min: Vector3<f32>,
    pub world_max: Vector3<f32>,
}

impl LightGrid {
    pub fn new(world_min: Vector3<f32>, world_max: Vector3<f32>, cell_size: f32) -> Self {
        let size = world_max - world_min;
        let grid_width = (size.x / cell_size).ceil() as u32;
        let grid_height = (size.y / cell_size).ceil() as u32;
        let grid_depth = (size.z / cell_size).ceil() as u32;
        let total_cells = (grid_width * grid_height * grid_depth) as usize;

        Self {
            cells: vec![crate::LightGridCell { offset: 0, count: 0 }; total_cells],
            entries: Vec::with_capacity(65536),
            grid_width,
            grid_height,
            grid_depth,
            cell_size,
            world_min,
            world_max,
        }
    }

    #[inline]
    pub fn get_cell_index(&self, pos: Vector3<f32>) -> Option<usize> {
        let local = pos - self.world_min;

        if local.x < 0.0 || local.y < 0.0 || local.z < 0.0 {
            return None;
        }

        let x = (local.x / self.cell_size) as u32;
        let y = (local.y / self.cell_size) as u32;
        let z = (local.z / self.cell_size) as u32;

        if x >= self.grid_width || y >= self.grid_height || z >= self.grid_depth {
            return None;
        }

        Some((z * self.grid_height * self.grid_width + y * self.grid_width + x) as usize)
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.offset = 0;
            cell.count = 0;
        }
        self.entries.clear();
    }

    pub fn add_light(&mut self, light_idx: u32, lod: u32, depth: f32, cell_idx: usize) {
        let entry = crate::LightGridEntry {
            light_index: light_idx,
            lod_level: lod,
            depth,
            padding: 0,
        };

        self.entries.push(entry);

        let cell = &mut self.cells[cell_idx];
        if cell.count == 0 {
            cell.offset = (self.entries.len() - 1) as u32;
        }
        cell.count += 1;
    }
}