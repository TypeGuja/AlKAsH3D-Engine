use crate::math::Vec3;

#[derive(Debug, Clone)]
pub struct Mesh {
    pub vertices: Vec<Vec3>,
    pub indices: Vec<u32>,
    pub normals: Vec<Vec3>,
    pub bounds: (Vec3, Vec3),
}

impl Mesh {
    pub fn new(vertices: Vec<Vec3>, indices: Vec<u32>) -> Self {
        let mut mesh = Self {
            vertices: vertices.clone(),
            indices: indices.clone(),
            normals: vec![Vec3::ZERO; vertices.len()],
            bounds: (Vec3::ZERO, Vec3::ZERO),
        };

        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        for v in &vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        mesh.bounds = (min, max);

        mesh.recalculate_normals();
        mesh
    }

    /// ДОБАВЛЕНО (редактор вершин/граней — по прямому запросу пользователя):
    /// пересчитывает `bounds` из ТЕКУЩЕГО `vertices` — раньше AABB считался
    /// только один раз в `new()`, что было корректно для мешей, которые
    /// после создания никогда не менялись. Редактирование вершин
    /// (move/extrude/delete — см. editor/mesh_edit.rs) меняет `vertices`
    /// уже ПОСЛЕ создания меша, так что без явного пересчёта bounds
    /// молчаливо устаревал бы (неверная bounding box в инспекторе/culling).
    pub fn recalculate_bounds(&mut self) {
        if self.vertices.is_empty() {
            self.bounds = (Vec3::ZERO, Vec3::ZERO);
            return;
        }
        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        for v in &self.vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        self.bounds = (min, max);
    }
}