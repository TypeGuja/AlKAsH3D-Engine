use nalgebra::Vector3;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LightType {
    Point = 0,
    Spot = 1,
    Directional = 2,
}

#[derive(Debug, Clone)]
pub struct Light {
    pub id: u32,
    pub position: Vector3<f32>,
    pub color: Vector3<f32>,
    pub intensity: f32,
    pub range: f32,
    pub light_type: LightType,
    pub spot_angle: f32,
    pub spot_direction: Vector3<f32>,
    pub falloff: f32,
    pub casts_shadows: bool,
    pub shadow_resolution: u32,
}

impl Light {
    pub fn point(pos: Vector3<f32>, color: Vector3<f32>, intensity: f32, range: f32) -> Self {
        Self {
            id: 0,
            position: pos,
            color,
            intensity,
            range,
            light_type: LightType::Point,
            spot_angle: std::f32::consts::PI,
            spot_direction: Vector3::new(0.0, -1.0, 0.0),
            falloff: 2.0,
            casts_shadows: false,
            shadow_resolution: 512,
        }
    }

    pub fn gpu_pack(&self) -> crate::GPULight {
        crate::GPULight {
            position: [self.position.x, self.position.y, self.position.z, self.light_type as u32 as f32],
            color: [self.color.x, self.color.y, self.color.z, self.intensity],
            direction: [self.spot_direction.x, self.spot_direction.y, self.spot_direction.z, self.range],
            params: [self.spot_angle, self.falloff, 0.0, 0.0],
        }
    }
}