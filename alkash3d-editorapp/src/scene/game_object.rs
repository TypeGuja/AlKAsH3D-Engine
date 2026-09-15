use std::collections::HashMap;
use uuid::Uuid;
use crate::math::{Vec3, Transform, Quat};
use crate::animation::AnimationTrack;
use super::object_type::ObjectType;

#[derive(Debug, Clone)]
pub struct GameObject {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub transform: Transform,
    pub object_type: ObjectType,
    pub animations: HashMap<String, Animation>,
    pub shader_technique: String,
    // ДОБАВЛЕНО (иерархия объектов — parent/child, как в Unity/Unreal):
    // родитель этого объекта в сцене, либо `None` для объекта верхнего
    // уровня. `transform` выше остаётся ЛОКАЛЬНЫМ (относительно родителя)
    // — мировой transform всегда получают через `Scene::get_world_transform`,
    // которая идёт вверх по цепочке `parent`. Список детей НЕ хранится
    // отдельным полем (не дублируем состояние, которое легко рассинхронить)
    // — `Scene::children_of` каждый раз сканирует объекты сцены по этому
    // полю; при масштабе сцен, с которыми работает этот эдитор, это дешевле
    // и надёжнее, чем поддерживать два источника истины.
    pub parent: Option<Uuid>,
}

#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub position_track: AnimationTrack<crate::math::Vec3>,
    pub rotation_track: AnimationTrack<crate::math::Quat>,
    pub scale_track: AnimationTrack<crate::math::Vec3>,
    pub duration: f32,
    pub playing: bool,
    pub current_time: f32,
    /// ДОБАВЛЕНО (по прямому запросу пользователя: показывать все
    /// анимационные точки во вьюпорте, чтобы их можно было двигать рукой)
    /// — переключается чекбоксом в `ui/inspector.rs`; пока `true`,
    /// `EditorApp::draw_keyframe_markers` рисует маркер на позиции каждого
    /// keyframe'а `position_track` и даёт их перетаскивать напрямую в
    /// вьюпорте (см. `EditorApp::handle_keyframe_marker_input`).
    pub show_keyframes: bool,
}

impl Animation {
    pub fn new(name: String) -> Self {
        Self {
            name,
            position_track: AnimationTrack::new(),
            rotation_track: AnimationTrack::new(),
            scale_track: AnimationTrack::new(),
            // ИСПРАВЛЕНО (баг: "можно поставить только 1 keyframe") — с
            // duration=0.0 слайдер Time в инспекторе (диапазон
            // `0.0..=duration.max(0.001)`) был фактически заблокирован
            // около нуля, так что все keyframe'ы попадали на одно и то же
            // время и второй просто ЗАМЕНЯЛ первый (см. `AnimationTrack::
            // add_keyframe` — совпадающее время обновляет существующий, а
            // не добавляет новый). Ненулевой дефолт даёт слайдеру реальный
            // диапазон сразу, без обязательного шага "сначала вручную
            // увеличь Duration".
            duration: 5.0,
            playing: false,
            current_time: 0.0,
            show_keyframes: false,
        }
    }

    pub fn update(&mut self, delta_time: f32) {
        if self.playing {
            self.current_time += delta_time;
            if self.current_time > self.duration {
                self.current_time = 0.0;
            }
        }
    }

    pub fn get_transform(&self) -> Transform {
        Transform {
            position: self.position_track.evaluate(self.current_time).unwrap_or(Vec3::ZERO),
            rotation: self.rotation_track.evaluate(self.current_time).unwrap_or(Quat::IDENTITY),
            scale: self.scale_track.evaluate(self.current_time).unwrap_or(Vec3::ONE),
        }
    }

    /// ДОБАВЛЕНО (проигрывание анимаций — см. `ui/inspector.rs` про
    /// авторинг keyframe'ов и `Scene::update` про сам плейбэк): в отличие
    /// от `get_transform()` выше (который для трека БЕЗ keyframes
    /// подставляет ZERO/IDENTITY/ONE), здесь трек без keyframes просто НЕ
    /// трогает соответствующий компонент `transform` — иначе анимация
    /// только позиции (обычный случай — двигать объект, не поворачивая и
    /// не масштабируя) сбрасывала бы ручной поворот/масштаб объекта в
    /// дефолт при каждом кадре проигрывания.
    pub fn apply_to_transform(&self, transform: &mut Transform) {
        if let Some(p) = self.position_track.evaluate(self.current_time) {
            transform.position = p;
        }
        if let Some(r) = self.rotation_track.evaluate(self.current_time) {
            transform.rotation = r;
        }
        if let Some(s) = self.scale_track.evaluate(self.current_time) {
            transform.scale = s;
        }
    }
}

impl GameObject {
    pub fn new(name: &str, object_type: ObjectType) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.to_string(),
            visible: true,
            locked: false,
            transform: Transform::default(),
            object_type,
            animations: HashMap::new(),
            shader_technique: "PBR_Standard".to_string(),
            parent: None,
        }
    }

    pub fn get_mesh_bounds(&self) -> Option<(Vec3, Vec3)> {
        match &self.object_type {
            ObjectType::Mesh(mesh_comp) => {
                let (min, max) = mesh_comp.mesh.bounds;
                let corners = [
                    Vec3::new(min.x, min.y, min.z),
                    Vec3::new(max.x, min.y, min.z),
                    Vec3::new(min.x, max.y, min.z),
                    Vec3::new(min.x, min.y, max.z),
                    Vec3::new(max.x, max.y, min.z),
                    Vec3::new(max.x, min.y, max.z),
                    Vec3::new(min.x, max.y, max.z),
                    Vec3::new(max.x, max.y, max.z),
                ];

                let mut world_min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
                let mut world_max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);

                for corner in &corners {
                    let world_corner = self.transform.transform_point(*corner);
                    world_min = world_min.min(world_corner);
                    world_max = world_max.max(world_corner);
                }

                Some((world_min, world_max))
            }
            _ => None,
        }
    }
}