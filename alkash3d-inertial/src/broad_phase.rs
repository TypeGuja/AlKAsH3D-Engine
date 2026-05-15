// src/broad_phase.rs
use rayon::prelude::*;
use crate::Vector3;  // 👈 ДОБАВИТЬ ЭТУ СТРОКУ

pub struct BroadPhase {
    grid: Vec<Vec<u32>>,
    cell_size: f32,
}

impl BroadPhase {
    pub fn new(world_size: f32, cell_size: f32) -> Self {
        let cells_x = (world_size / cell_size).ceil() as usize;
        let cells_z = cells_x;
        let total_cells = cells_x * cells_z;

        Self {
            grid: vec![Vec::new(); total_cells],
            cell_size,
        }
    }

    #[inline(always)]
    pub fn find_pairs_parallel(&mut self, positions: &[Vector3]) -> Vec<(u32, u32)> {
        // Очистка сетки
        self.grid.par_iter_mut().for_each(|cell| cell.clear());

        // Параллельное заполнение
        positions.par_iter().enumerate().for_each(|(idx, pos)| {
            let cell = self.get_cell(pos);
            if cell < self.grid.len() {
                unsafe {
                    self.grid.get_unchecked_mut(cell).push(idx as u32);
                }
            }
        });

        // Параллельный поиск пар
        self.grid
            .par_iter()
            .flat_map(|cell| {
                let mut pairs = Vec::with_capacity(cell.len() * 2);
                for i in 0..cell.len() {
                    for j in i + 1..cell.len() {
                        pairs.push((cell[i], cell[j]));
                    }
                }
                pairs
            })
            .collect()
    }

    #[inline(always)]
    fn get_cell(&self, pos: &Vector3) -> usize {
        let x = (pos.x / self.cell_size).floor() as i32;
        let z = (pos.z / self.cell_size).floor() as i32;
        let cells_per_axis = (self.grid.len() as f32).sqrt() as i32;
        ((x & 0xFFFF) ^ ((z & 0xFFFF) << 16)) as usize % self.grid.len()
    }
}