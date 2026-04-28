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