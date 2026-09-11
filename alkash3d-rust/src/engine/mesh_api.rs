//! Публичный API добавления мешей в движок (`add_mesh`/`add_cube`/
//! `add_box_textured`/...) и API спавна ECS-сущностей со `MeshRenderer`
//! (`spawn_mesh_entity`/`spawn_static_mesh`/`spawn_child_mesh`/...).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись — в оригинале эти две группы методов были
//! разнесены по разным частям одного гигантского `impl AlkashEngine`
//! (одна — рядом с `AlkashEngine::new`, другая — рядом с `render_frame`),
//! здесь они объединены логически, т.к. обе относятся к одному и тому же
//! "API геометрии сцены".

use super::{AlkashEngine, Mesh, LodGroup};

impl AlkashEngine {
    /// Удобный конструктор: создаёт сущность в ECS-сцене и сразу вешает на
    /// неё `MeshRenderer`, ссылающийся на уже загруженный меш (индекс из
    /// `add_cube`/`add_quad`/`add_mesh`/... — то же самое хранилище, что
    /// используется старым `MeshInstance`-путём).
    pub fn spawn_mesh_entity(&mut self, mesh_index: usize) -> crate::scene::EntityId {
        let id = self.scene.spawn();
        self.scene.add_mesh_renderer(id, mesh_index);
        id
    }

    /// ДОБАВЛЕНО (по просьбе пользователя — весь доступ к внутренностям
    /// движка из bin-файлов должен идти через хелперы главного файла
    /// движка, а не напрямую через `engine.scene.*`, чтобы bin-файлы не
    /// могли случайно нарушить инварианты ECS-сцены): объединяет
    /// `spawn_mesh_entity` + позиционирование (`scene.transform_mut`) в
    /// один вызов — типичный случай "заспавнить статичный меш сразу с
    /// известными position/scale" (плитка пола, стена и т.п.), которым
    /// раньше bin-файлы занимались сами через прямой доступ к `engine.scene`.
    /// `rotation` — Эйлеровы углы в радианах (см. `Transform::rotation`),
    /// `[0.0, 0.0, 0.0]` для большинства статичной геометрии без наклона.
    pub fn spawn_static_mesh(
        &mut self,
        mesh_index: usize,
        position: [f32; 3],
        rotation: [f32; 3],
        scale: [f32; 3],
    ) -> crate::scene::EntityId {
        let id = self.spawn_mesh_entity(mesh_index);
        if let Some(t) = self.scene.transform_mut(id) {
            t.position = position;
            t.rotation = rotation;
            t.scale = scale;
        }
        id
    }

    /// ДОБАВЛЕНО (см. `spawn_static_mesh` выше): число ECS-сущностей в
    /// сцене — без этого bin-файлам приходилось читать `engine.scene.len()`
    /// напрямую только ради счётчика в диагностическом выводе.
    pub fn scene_entity_count(&self) -> usize {
        self.scene.len()
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — колёса машины, дочерние
    /// сущности): та же идея, что и у `spawn_static_mesh` (bin-файлы не
    /// трогают `engine.scene` напрямую — см. комментарий там), но для
    /// ДОЧЕРНЕГО меша: сразу вызывает `scene.set_parent`, чтобы вызывающему
    /// коду не пришлось делать два отдельных вызова (`spawn_mesh_entity` +
    /// `set_parent`) и не забыть про один из них. `local_pos`/`local_rot` —
    /// координаты ОТНОСИТЕЛЬНО родителя (как и `Transform.position` любой
    /// дочерней сущности, см. `Scene::walk`), а не мировые.
    pub fn spawn_child_mesh(
        &mut self,
        mesh_index: usize,
        parent: crate::scene::EntityId,
        local_pos: [f32; 3],
        local_rot: [f32; 3],
        local_scale: [f32; 3],
    ) -> crate::scene::EntityId {
        let id = self.spawn_mesh_entity(mesh_index);
        self.scene.set_parent(id, Some(parent));
        if let Some(t) = self.scene.transform_mut(id) {
            t.position = local_pos;
            t.rotation = local_rot;
            t.scale = local_scale;
        }
        id
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — управляемая машина): обновляет
    /// position/rotation уже существующей сущности каждый кадр (машина
    /// едет не через физику Inertial, а через собственную аркадную
    /// симуляцию в `car_sim.rs` — см. подробное обоснование в шапке того
    /// файла — поэтому нужен способ каждый кадр записать её результат в
    /// Transform сущности, не трогая `engine.scene` напрямую из bin-файла).
    pub fn set_entity_transform(&mut self, id: crate::scene::EntityId, position: [f32; 3], rotation: [f32; 3]) {
        if let Some(t) = self.scene.transform_mut(id) {
            t.position = position;
            t.rotation = rotation;
        }
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо): мировая позиция сущности из её
    /// (локального, если есть родитель) `Transform.position` — для
    /// корневых сущностей (машина, игрок) это одно и то же; используется,
    /// например, чтобы проверить, достаточно ли игрок близко к машине для
    /// "сесть/выйти" (см. `main_car.rs`).
    pub fn entity_position(&self, id: crate::scene::EntityId) -> Option<[f32; 3]> {
        self.scene.transform(id).map(|t| t.position)
    }

    pub fn add_mesh(&mut self, mesh: Mesh) -> usize {
        self.meshes.push(mesh);
        println!("[ENGINE] Mesh added, total meshes: {}", self.meshes.len());
        self.meshes.len() - 1
    }

    pub fn add_triangle(&mut self) -> usize {
        let mesh = Mesh::triangle().unwrap();
        self.add_mesh(mesh)
    }

    pub fn add_quad(&mut self, x: f32, y: f32, width: f32, height: f32, color: [f32; 4]) -> usize {
        let mesh = Mesh::quad(x, y, width, height, color).unwrap();
        self.add_mesh(mesh)
    }

    pub fn add_cube(&mut self, size: f32) -> usize {
        let mesh = Mesh::cube(size).unwrap();
        self.add_mesh(mesh)
    }

    /// См. подробное объяснение у `Mesh::cube_colored` — куб с ОДНИМ
    /// нейтральным цветом на всех гранях (в отличие от `add_cube`,
    /// который красит каждую грань в отдельный отладочный цвет), нужен
    /// для геометрии, где важно реально увидеть результат освещения
    /// (point/spot-фонари, тени), а не отладочную раскраску нормалей.
    pub fn add_cube_colored(&mut self, size: f32, r: f32, g: f32, b: f32, a: f32) -> usize {
        let mesh = Mesh::cube_colored(size, r, g, b, a).unwrap();
        self.add_mesh(mesh)
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — текстуры): `Mesh::box_textured`
    /// + назначение материала (albedo SRV из `create_texture_rgba`, PBR
    /// скаляры) одним вызовом — тот же паттерн, что у `add_cube_colored`
    /// выше, только для текстурированной геометрии. `albedo_srv` — `None`
    /// даёт обычный нейтральный белый fallback (см. `Mesh::albedo_srv_index`)
    /// — то есть просто однотонный `tint`, без текстуры вообще, если она
    /// не нужна.
    pub fn add_box_textured(
        &mut self,
        half_extents: [f32; 3],
        uv_scale: f32,
        tint: [f32; 4],
        albedo_srv: Option<u32>,
        roughness: f32,
        metallic: f32,
    ) -> usize {
        let mut mesh = Mesh::box_textured(half_extents, uv_scale, tint).unwrap();
        mesh.albedo_srv_index = albedo_srv;
        mesh.material_roughness = roughness;
        mesh.material_metallic = metallic;
        self.add_mesh(mesh)
    }

    /// См. `add_box_textured` выше — то же самое, для `Mesh::plane_textured`
    /// (большая плоскость земли/двора одним draw call'ом).
    pub fn add_plane_textured(
        &mut self,
        width: f32,
        depth: f32,
        uv_scale: f32,
        tint: [f32; 4],
        albedo_srv: Option<u32>,
        roughness: f32,
        metallic: f32,
    ) -> usize {
        let mut mesh = Mesh::plane_textured(width, depth, uv_scale, tint).unwrap();
        mesh.albedo_srv_index = albedo_srv;
        mesh.material_roughness = roughness;
        mesh.material_metallic = metallic;
        self.add_mesh(mesh)
    }

    /// См. `add_box_textured` выше — то же самое, для `Mesh::cylinder_textured`
    /// (колёса машины).
    pub fn add_cylinder_textured(
        &mut self,
        radius: f32,
        width: f32,
        segments: u32,
        uv_scale: f32,
        tint: [f32; 4],
        albedo_srv: Option<u32>,
        roughness: f32,
        metallic: f32,
    ) -> usize {
        let mut mesh = Mesh::cylinder_textured(radius, width, segments, uv_scale, tint).unwrap();
        mesh.albedo_srv_index = albedo_srv;
        mesh.material_roughness = roughness;
        mesh.material_metallic = metallic;
        self.add_mesh(mesh)
    }

    pub fn clear_meshes(&mut self) {
        self.meshes.clear();
        self.mesh_instances.clear();
        self.lod_groups.clear();
        println!("[ENGINE] All meshes and instances cleared");
    }

    /// ДОБАВЛЕНО (максимальная графика — LOD для мешей): регистрирует
    /// группу уровней детализации — `mesh_indices[0]` (LOD0, обычно самый
    /// детальный) становится "логическим" индексом объекта: везде, где
    /// игровой код уже передаёт `mesh_indices[0]` в `MeshInstance::new`,
    /// `spawn_static_mesh` и т.п., `render_frame` теперь САМ подменит его
    /// на подходящий по расстоянию до камеры уровень (см.
    /// `resolve_lod_mesh_index` в render_frame.rs) — существующий игровой
    /// код МЕНЯТЬ НЕ НУЖНО, только один раз вызвать этот метод.
    ///
    /// `mesh_indices`/`distances` — параллельные массивы одинаковой длины,
    /// `distances` СТРОГО по возрастанию (`distances[i]` — максимальное
    /// расстояние, на котором ещё используется `mesh_indices[i]`). Объект
    /// дальше `distances[distances.len()-1]` не рисуется вовсе — передай
    /// `f32::MAX` последним порогом, если такое отсечение не нужно (см.
    /// `LodGroup::resolve`). Молча ничего не делает (с диагностикой в
    /// stderr) при пустых/разной длины массивах или индексах вне
    /// `self.meshes` — тот же принцип "не паникуем на плохом вводе", что и
    /// у остального публичного API этого файла.
    pub fn add_lod_group(&mut self, mesh_indices: Vec<usize>, distances: Vec<f32>) {
        if mesh_indices.is_empty() || mesh_indices.len() != distances.len() {
            eprintln!(
                "[ENGINE] add_lod_group: mesh_indices ({}) и distances ({}) должны быть непустыми и одинаковой длины — группа не добавлена",
                mesh_indices.len(), distances.len()
            );
            return;
        }
        if let Some(&bad) = mesh_indices.iter().find(|&&i| i >= self.meshes.len()) {
            eprintln!(
                "[ENGINE] add_lod_group: индекс меша {} вне диапазона (мешей всего {}) — группа не добавлена",
                bad, self.meshes.len()
            );
            return;
        }
        for w in distances.windows(2) {
            if w[1] <= w[0] {
                eprintln!("[ENGINE] add_lod_group: distances должны строго возрастать — группа не добавлена");
                return;
            }
        }
        let key = mesh_indices[0];
        self.lod_groups.insert(key, LodGroup { mesh_indices, distances });
        println!("[ENGINE] ✓ LOD-группа добавлена для меша #{}", key);
    }
}
