use crate::mesh::Mesh;
use crate::material::Material;
use crate::particle::ParticleSystem;

#[derive(Debug, Clone)]
pub enum ObjectType {
    Empty,
    Mesh(MeshComponent),
    Light(LightComponent),
    Camera(CameraComponent),
    ParticleSystem(ParticleSystemComponent),
    AudioSource(AudioSourceComponent),
    ScriptedEntity(ScriptedEntityComponent),
    /// ДОБАВЛЕНО (точка спавна игрока — по прямому запросу пользователя):
    /// позиция и направление взгляда игрока при появлении в мире —
    /// позиция берётся из `GameObject::transform.position`, направление
    /// взгляда — из `transform.rotation.forward()` (тот же паттерн, что и
    /// у Light: ориентация не хранится отдельным полем, целиком выражена
    /// через transform самого объекта). Пустой вариант — как у `Empty`,
    /// сам факт "это SpawnPoint" и есть вся нужная информация сверх
    /// transform. Экспортируется в `.alworld` как `GlobalObject` с
    /// зарезервированным битом `GLOBAL_OBJECT_FLAG_SPAWN_POINT` (см.
    /// `converters/alworld.rs`) — движок сейчас `global_objects` вообще не
    /// читает (см. поиск по engine/*.rs), так что это чисто аддитивный,
    /// ничего не ломающий способ протащить точку спавна через уже
    /// существующее, но пока не используемое поле формата.
    SpawnPoint,
}

#[derive(Debug, Clone)]
pub struct MeshComponent {
    pub mesh: Mesh,
    pub material: Material,
    pub visible: bool,
    pub wireframe: bool,
    pub solid: bool,
    pub double_sided: bool,
}

#[derive(Debug, Clone)]
pub struct LightComponent {
    pub light_type: LightType,
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub enabled: bool,
    /// ДОБАВЛЕНО (по прямому запросу пользователя: "id света чтобы каждый
    /// раз на свет не загружать almat для нужного оттенка") — ссылка на
    /// переиспользуемый оттенок (`crate::assets::LightColorGroup`) по
    /// числовому ID вместо копии цвета/интенсивности. `color`/`intensity`
    /// выше остаются РЕАЛЬНЫМИ значениями, которые читает рендер/экспорт в
    /// .alfar (ничего в этих системах не пришлось менять) — назначение
    /// группы копирует её цвет/интенсивность сюда один раз, а последующая
    /// правка самой группы рассылается по всем светильникам с этим ID через
    /// `EditorApp::sync_light_color_group`, а не хранится как "живая"
    /// ссылка, которую пришлось бы резолвить на каждом кадре.
    pub color_group: Option<u32>,
}

#[derive(Debug, Clone)]
pub enum LightType {
    Point,
    Directional,
    Spot { inner_angle: f32, outer_angle: f32 },
}

#[derive(Debug, Clone)]
pub struct CameraComponent {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
    pub orthographic: bool,
}

#[derive(Debug, Clone)]
pub struct ParticleSystemComponent {
    pub system: ParticleSystem,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct AudioSourceComponent {
    pub sound_name: String,
    pub volume: f32,
    pub spatial_blend: f32,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct ScriptedEntityComponent {
    pub script_name: String,
    pub enabled: bool,
}