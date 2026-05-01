// PBR Shader
struct Camera {
    view_proj: mat4x4<f32>,
    view_position: vec3<f32>,
}

struct Light {
    position: vec3<f32>,
    intensity: f32,
    color: vec3<f32>,
    range: f32,
}

struct Material {
    albedo: vec4<f32>,
    metallic: f32,
    roughness: f32,
    ao: f32,
}

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var<uniform> light: Light;

@group(2) @binding(0)
var<uniform> material: Material;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct InstanceInput {
    @location(5) model_col0: vec4<f32>,
    @location(6) model_col1: vec4<f32>,
    @location(7) model_col2: vec4<f32>,
    @location(8) model_col3: vec4<f32>,
    @location(9) material_id: u32,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vs_main(
    in: VertexInput,
    instance: InstanceInput,
) -> VertexOutput {
    var out: VertexOutput;

    let model_matrix = mat4x4<f32>(
        instance.model_col0,
        instance.model_col1,
        instance.model_col2,
        instance.model_col3,
    );

    let world_position = model_matrix * vec4<f32>(in.position, 1.0);
    out.world_position = world_position.xyz;
    out.world_normal = normalize((model_matrix * vec4<f32>(in.normal, 0.0)).xyz);
    out.uv = in.uv;
    out.clip_position = camera.view_proj * world_position;

    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let l = normalize(light.position - in.world_position);
    let v = normalize(camera.view_position - in.world_position);
    let h = normalize(l + v);

    // Diffuse
    let ndotl = max(dot(n, l), 0.0);
    let diffuse = material.albedo.rgb * ndotl * light.color * light.intensity;

    // Ambient
    let ambient = material.albedo.rgb * 0.1 * material.ao;

    // Specular (Blinn-Phong)
    let specular = pow(max(dot(n, h), 0.0), 32.0) * 0.5;

    let final_color = (diffuse + ambient + specular) * material.albedo.a;

    return vec4<f32>(final_color, material.albedo.a);
}