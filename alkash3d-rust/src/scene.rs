// src/scene.rs
//! Сцена / ECS-ядро движка.
//!
//! Простой, но надёжный ECS на основе generational indices ("sparse set"
//! стиль, не архетипный — этого достаточно для сотен-тысяч объектов и
//! гораздо проще в реализации/отладке, чем полноценный архетипный ECS).
//!
//! Даёт то, чего не было у `Vec<MeshInstance>`:
//!  - стабильные ID сущностей, которые НЕ сдвигаются при удалении других
//!    объектов (в `Vec<MeshInstance>` индекс = позиция в массиве, поэтому
//!    удаление элемента из середины валит все ссылки на "то, что после
//!    него" — тут такой проблемы нет в принципе);
//!  - защиту от use-after-free на уровне API: если сущность удалена, а
//!    где-то ещё лежит её старый `EntityId` — обращение к нему просто
//!    вернёт `None`, а не прочитает данные чужой сущности, случайно
//!    занявшей тот же слот (это и есть генерация — `generation`);
//!  - иерархию parent/child с вычислением мировых трансформаций обходом
//!    от корней;
//!  - независимые, точечно добавляемые/удаляемые компоненты вместо одной
//!    монолитной структуры "на всё".
//!
//! ВАЖНО: этот модуль ДОБАВЛЯЕТСЯ к существующему движку и НИЧЕГО не
//! ломает. `AlkashEngine::mesh_instances` / `AlkashEngine::meshes`
//! продолжают работать в точности как раньше — `render_frame()`
//! дополнительно (не вместо) рендерит сущности из `AlkashEngine::scene`,
//! если они там есть. Можно продолжать использовать старый API, можно
//! постепенно перейти на ECS, можно использовать оба одновременно.

use crate::math::{identity, Mat4, Vec3};

/// Генерационный индекс сущности: (индекс слота, поколение).
///
/// Поколение инкрементируется при каждом `despawn` данного слота — поэтому
/// старый `EntityId`, полученный до удаления, никогда не совпадёт с новым
/// поколением того же слота, даже если слот переиспользован под другую
/// сущность. Это и есть защита от "тихого" use-after-free на уровне API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EntityId {
    index: u32,
    generation: u32,
}

impl EntityId {
    /// Заведомо невалидный ID — удобно как значение по умолчанию там, где
    /// нужен "пока не назначено" без `Option`.
    pub const INVALID: EntityId = EntityId {
        index: u32::MAX,
        generation: u32::MAX,
    };

    #[inline]
    pub fn index(&self) -> u32 {
        self.index
    }

    #[inline]
    pub fn generation(&self) -> u32 {
        self.generation
    }
}

impl Default for EntityId {
    fn default() -> Self {
        Self::INVALID
    }
}

struct Slot {
    generation: u32,
    alive: bool,
}

/// Компонент трансформации + место сущности в иерархии.
///
/// Есть у КАЖДОЙ живой сущности (создаётся автоматически в `Scene::spawn`)
/// — без трансформации не имеет смысла ни иерархия, ни рендер, так что это
/// не опциональный компонент, а минимальный контракт сцены.
#[derive(Debug, Clone)]
pub struct Transform {
    pub position: [f32; 3],
    /// Эйлеровы углы в порядке ZYX — так же, как раньше в `MeshInstance`,
    /// чтобы старые сцены на mesh_instances и новые на Scene визуально
    /// вели себя одинаково.
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub parent: Option<EntityId>,
    pub children: Vec<EntityId>,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            scale: [1.0, 1.0, 1.0],
            parent: None,
            children: Vec::new(),
        }
    }
}

impl Transform {
    /// Локальная матрица трансформации (относительно родителя).
    pub fn local_matrix(&self) -> Mat4 {
        let t = Mat4::from_translation(Vec3::new(
            self.position[0],
            self.position[1],
            self.position[2],
        ));
        let rz = Mat4::from_rotation_z(self.rotation[2]);
        let ry = Mat4::from_rotation_y(self.rotation[1]);
        let rx = Mat4::from_rotation_x(self.rotation[0]);
        let r = rz * ry * rx;
        let s = Mat4::from_scale(Vec3::new(self.scale[0], self.scale[1], self.scale[2]));
        t * r * s
    }
}

/// Компонент рендеринга. Ссылается на индекс в `AlkashEngine::meshes` —
/// сознательно переиспользуем уже существующее хранилище GPU-мешей,
/// вместо того чтобы дублировать вершинные буферы под ECS отдельно.
#[derive(Debug, Clone, Copy)]
pub struct MeshRenderer {
    pub mesh_index: usize,
    pub visible: bool,
}

/// Компонент источника света (метаданные на стороне сцены; фактическая
/// GPU-структура света для LightPlugin собирается отдельно, при синхронизации
/// сцены со светом — см. `Scene::for_each_world_transform` + компонент `light`).
#[derive(Debug, Clone, Copy)]
pub struct LightComponent {
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Name(pub String);

/// Разреженное хранилище одного типа компонента, индексированное по
/// индексу слота сущности. Поколение (generation) тут не нужно — оно
/// проверяется один раз на уровне `Scene` перед любым доступом.
struct ComponentStorage<T> {
    data: Vec<Option<T>>,
}

impl<T> ComponentStorage<T> {
    fn new() -> Self {
        Self { data: Vec::new() }
    }

    fn ensure_len(&mut self, len: usize) {
        if self.data.len() < len {
            self.data.resize_with(len, || None);
        }
    }

    fn insert(&mut self, index: u32, value: T) {
        self.ensure_len(index as usize + 1);
        self.data[index as usize] = Some(value);
    }

    fn remove(&mut self, index: u32) -> Option<T> {
        self.data.get_mut(index as usize).and_then(|slot| slot.take())
    }

    fn get(&self, index: u32) -> Option<&T> {
        self.data.get(index as usize).and_then(|s| s.as_ref())
    }

    fn get_mut(&mut self, index: u32) -> Option<&mut T> {
        self.data.get_mut(index as usize).and_then(|s| s.as_mut())
    }
}

/// Сцена: держит все сущности и их компоненты.
pub struct Scene {
    slots: Vec<Slot>,
    free_list: Vec<u32>,
    /// Сущности без родителя — точки входа для обхода иерархии.
    roots: Vec<EntityId>,

    transforms: ComponentStorage<Transform>,
    mesh_renderers: ComponentStorage<MeshRenderer>,
    lights: ComponentStorage<LightComponent>,
    names: ComponentStorage<Name>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            slots: Vec::new(),
            free_list: Vec::new(),
            roots: Vec::new(),
            transforms: ComponentStorage::new(),
            mesh_renderers: ComponentStorage::new(),
            lights: ComponentStorage::new(),
            names: ComponentStorage::new(),
        }
    }

    /// Создаёт новую сущность с дефолтным `Transform` (корневую — без
    /// родителя). Возвращает стабильный `EntityId`.
    pub fn spawn(&mut self) -> EntityId {
        let id = if let Some(index) = self.free_list.pop() {
            let slot = &mut self.slots[index as usize];
            slot.alive = true;
            EntityId {
                index,
                generation: slot.generation,
            }
        } else {
            let index = self.slots.len() as u32;
            self.slots.push(Slot {
                generation: 0,
                alive: true,
            });
            EntityId {
                index,
                generation: 0,
            }
        };

        self.transforms.insert(id.index, Transform::default());
        self.roots.push(id);
        id
    }

    /// Жива ли сущность (не была ли удалена, и не переиспользован ли её
    /// слот под другую сущность после `despawn`).
    #[inline]
    pub fn is_alive(&self, id: EntityId) -> bool {
        self.slots
            .get(id.index as usize)
            .map(|s| s.alive && s.generation == id.generation)
            .unwrap_or(false)
    }

    /// Удаляет сущность и РЕКУРСИВНО всех её потомков — иначе они остались
    /// бы висеть с `parent`, указывающим на мёртвый слот.
    pub fn despawn(&mut self, id: EntityId) {
        if !self.is_alive(id) {
            return;
        }

        let children = self
            .transforms
            .get(id.index)
            .map(|t| t.children.clone())
            .unwrap_or_default();
        for child in children {
            self.despawn(child);
        }

        // Отвязываем от родителя (или из списка корней).
        let parent = self.transforms.get(id.index).and_then(|t| t.parent);
        match parent {
            Some(p) => {
                if let Some(parent_transform) = self.transforms.get_mut(p.index) {
                    parent_transform.children.retain(|c| *c != id);
                }
            }
            None => self.roots.retain(|r| *r != id),
        }

        self.transforms.remove(id.index);
        self.mesh_renderers.remove(id.index);
        self.lights.remove(id.index);
        self.names.remove(id.index);

        let slot = &mut self.slots[id.index as usize];
        slot.alive = false;
        slot.generation = slot.generation.wrapping_add(1);
        self.free_list.push(id.index);
    }

    /// Меняет родителя сущности (`None` — сделать корневой).
    ///
    /// Защищено от создания циклов в иерархии: если `new_parent` на самом
    /// деле является потомком `id`, вызов молча игнорируется — иначе обход
    /// иерархии (`for_each_world_transform`) ушёл бы в бесконечную
    /// рекурсию.
    pub fn set_parent(&mut self, id: EntityId, new_parent: Option<EntityId>) {
        if !self.is_alive(id) {
            return;
        }
        if let Some(p) = new_parent {
            if !self.is_alive(p) {
                return;
            }
            // Проверяем всю цепочку родителей p вверх до корня: если id
            // встретится среди предков p, значит p — потомок id, и такое
            // назначение создало бы цикл.
            let mut cursor = Some(p);
            while let Some(c) = cursor {
                if c == id {
                    return;
                }
                cursor = self.transforms.get(c.index).and_then(|t| t.parent);
            }
        }

        let old_parent = self.transforms.get(id.index).and_then(|t| t.parent);
        if old_parent == new_parent {
            return;
        }

        match old_parent {
            Some(op) => {
                if let Some(t) = self.transforms.get_mut(op.index) {
                    t.children.retain(|c| *c != id);
                }
            }
            None => self.roots.retain(|r| *r != id),
        }

        if let Some(t) = self.transforms.get_mut(id.index) {
            t.parent = new_parent;
        }

        match new_parent {
            Some(np) => {
                if let Some(t) = self.transforms.get_mut(np.index) {
                    t.children.push(id);
                }
            }
            None => self.roots.push(id),
        }
    }

    pub fn transform(&self, id: EntityId) -> Option<&Transform> {
        if !self.is_alive(id) {
            return None;
        }
        self.transforms.get(id.index)
    }

    pub fn transform_mut(&mut self, id: EntityId) -> Option<&mut Transform> {
        if !self.is_alive(id) {
            return None;
        }
        self.transforms.get_mut(id.index)
    }

    pub fn add_mesh_renderer(&mut self, id: EntityId, mesh_index: usize) {
        if self.is_alive(id) {
            self.mesh_renderers.insert(
                id.index,
                MeshRenderer {
                    mesh_index,
                    visible: true,
                },
            );
        }
    }

    pub fn mesh_renderer(&self, id: EntityId) -> Option<&MeshRenderer> {
        if !self.is_alive(id) {
            return None;
        }
        self.mesh_renderers.get(id.index)
    }

    pub fn mesh_renderer_mut(&mut self, id: EntityId) -> Option<&mut MeshRenderer> {
        if !self.is_alive(id) {
            return None;
        }
        self.mesh_renderers.get_mut(id.index)
    }

    pub fn remove_mesh_renderer(&mut self, id: EntityId) {
        if self.is_alive(id) {
            self.mesh_renderers.remove(id.index);
        }
    }

    pub fn add_light(&mut self, id: EntityId, light: LightComponent) {
        if self.is_alive(id) {
            self.lights.insert(id.index, light);
        }
    }

    pub fn light(&self, id: EntityId) -> Option<&LightComponent> {
        if !self.is_alive(id) {
            return None;
        }
        self.lights.get(id.index)
    }

    pub fn set_name(&mut self, id: EntityId, name: impl Into<String>) {
        if self.is_alive(id) {
            self.names.insert(id.index, Name(name.into()));
        }
    }

    pub fn name(&self, id: EntityId) -> Option<&str> {
        if !self.is_alive(id) {
            return None;
        }
        self.names.get(id.index).map(|n| n.0.as_str())
    }

    /// Количество живых сущностей.
    pub fn len(&self) -> usize {
        self.slots.iter().filter(|s| s.alive).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Обходит иерархию от корней вниз, вызывая `f(id, world_matrix)` для
    /// каждой живой сущности. Мировая матрица — произведение локальных
    /// матриц по цепочке родителей (корень → ... → сущность).
    pub fn for_each_world_transform<F: FnMut(EntityId, Mat4)>(&self, mut f: F) {
        let root_matrix = identity();
        for &root in &self.roots {
            self.walk(root, root_matrix, &mut f);
        }
    }

    fn walk<F: FnMut(EntityId, Mat4)>(&self, id: EntityId, parent_world: Mat4, f: &mut F) {
        let Some(t) = self.transforms.get(id.index) else {
            return;
        };
        let world = parent_world * t.local_matrix();
        f(id, world);
        for &child in &t.children {
            self.walk(child, world, f);
        }
    }

    /// Удобный метод для рендерера: собрать `(mesh_index, world_matrix)`
    /// для всех видимых `MeshRenderer` во всей иерархии сцены за один
    /// проход.
    pub fn collect_renderables(&self) -> Vec<(usize, Mat4)> {
        let mut out = Vec::new();
        self.for_each_world_transform(|id, world| {
            if let Some(mr) = self.mesh_renderers.get(id.index) {
                if mr.visible {
                    out.push((mr.mesh_index, world));
                }
            }
        });
        out
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawn_despawn_generation() {
        let mut scene = Scene::new();
        let a = scene.spawn();
        assert!(scene.is_alive(a));
        scene.despawn(a);
        assert!(!scene.is_alive(a));

        // Новая сущность может переиспользовать тот же слот, но со старым
        // EntityId она не должна совпадать (разная generation).
        let b = scene.spawn();
        assert_eq!(a.index(), b.index());
        assert_ne!(a.generation(), b.generation());
        assert!(!scene.is_alive(a));
        assert!(scene.is_alive(b));
    }

    #[test]
    fn hierarchy_cycle_is_rejected() {
        let mut scene = Scene::new();
        let parent = scene.spawn();
        let child = scene.spawn();
        scene.set_parent(child, Some(parent));

        // Попытка сделать parent потомком child создала бы цикл — должна
        // быть проигнорирована.
        scene.set_parent(parent, Some(child));
        assert_eq!(scene.transform(parent).unwrap().parent, None);
    }

    #[test]
    fn despawn_removes_children_too() {
        let mut scene = Scene::new();
        let parent = scene.spawn();
        let child = scene.spawn();
        scene.set_parent(child, Some(parent));

        scene.despawn(parent);
        assert!(!scene.is_alive(parent));
        assert!(!scene.is_alive(child));
    }
}
