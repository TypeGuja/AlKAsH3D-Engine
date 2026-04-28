// PBR Shader

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) tex_coord: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) world_position: vec3<f32>,
    @location(1) world_normal: vec3<f32>,
    @location(2) tex_coord: vec2<f32>,
}

struct CameraUniform {
    view_proj: mat4x4<f32>,
    view_position: vec3<f32>,
    _padding: f32,
}

struct MaterialUniform {
    albedo: vec4<f32>,
    metallic: f32,
    roughness: f32,
    ao: f32,
}

struct LightUniform {
    position: vec3<f32>,
    intensity: f32,
    color: vec3<f32>,
    range: f32,
}

struct LightArray {
    lights: array<LightUniform, 16>,
    count: u32,
    _padding: vec3<u32>,
}

@group(0) @binding(0)
var<uniform> camera: CameraUniform;

@group(1) @binding(0)
var<uniform> material: MaterialUniform;

@group(2) @binding(0)
var<uniform> light_uniforms: LightArray;

const PI: f32 = 3.14159265359;

fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let n_dot_h2 = n_dot_h * n_dot_h;
    let nom = a2;
    var denom = (n_dot_h2 * (a2 - 1.0) + 1.0);
    denom = PI * denom * denom;
    return nom / denom;
}

fn geometry_schlick_ggx(n_dot_v: f32, roughness: f32) -> f32 {
    let r = roughness + 1.0;
    let k = (r * r) / 8.0;
    let nom = n_dot_v;
    let denom = n_dot_v * (1.0 - k) + k;
    return nom / denom;
}

fn geometry_smith(n: vec3<f32>, v: vec3<f32>, l: vec3<f32>, roughness: f32) -> f32 {
    let n_dot_v = max(dot(n, v), 0.0);
    let n_dot_l = max(dot(n, l), 0.0);
    let ggx2 = geometry_schlick_ggx(n_dot_v, roughness);
    let ggx1 = geometry_schlick_ggx(n_dot_l, roughness);
    return ggx1 * ggx2;
}

fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.world_position = in.position;
    out.world_normal = in.normal;
    out.tex_coord = in.tex_coord;
    out.clip_position = camera.view_proj * vec4<f32>(in.position, 1.0);
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.world_normal);
    let v = normalize(camera.view_position - in.world_position);

    let f0 = mix(vec3<f32>(0.04), material.albedo.rgb, material.metallic);

    var lo = vec3<f32>(0.0);

    for (var i: u32 = 0u; i < light_uniforms.count; i++) {
        let light = light_uniforms.lights[i];
        let l = normalize(light.position - in.world_position);
        let h = normalize(v + l);

        let distance = length(light.position - in.world_position);
        let attenuation = 1.0 / (distance * distance);
        let radiance = light.color * light.intensity * attenuation;

        let ndf = distribution_ggx(n, h, material.roughness);
        let g = geometry_smith(n, v, l, material.roughness);
        let f = fresnel_schlick(clamp(dot(h, v), 0.0, 1.0), f0);

        let numerator = ndf * g * f;
        let denominator = 4.0 * max(dot(n, v), 0.0) * max(dot(n, l), 0.0) + 0.0001;
        let specular = numerator / denominator;

        let kd = (vec3<f32>(1.0) - f) * (1.0 - material.metallic);
        let n_dot_l = max(dot(n, l), 0.0);

        lo += (kd * material.albedo.rgb / PI + specular) * radiance * n_dot_l;
    }

    let ambient = vec3<f32>(0.03) * material.albedo.rgb * material.ao;
    var color = ambient + lo;

    color = color / (color + vec3<f32>(1.0));
    color = pow(color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(color, material.albedo.a);
}