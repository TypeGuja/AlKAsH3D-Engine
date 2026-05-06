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

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var<uniform> light: Light;
@group(2) @binding(0) var<uniform> material: Material;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) texcoord: vec2<f32>,
    @location(3) color: vec4<f32>,
}

struct VertexOutput {
    @builtin(position) clip_pos: vec4<f32>,
    @location(0) world_pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) texcoord: vec2<f32>,
    @location(3) color: vec4<f32>,
}

// Push constants: model matrix
var<push_constant> model: mat4x4<f32>;

fn calculate_normal_matrix(m: mat4x4<f32>) -> mat4x4<f32> {
    return mat4x4<f32>(
        vec4<f32>(m[0].xyz, 0.0),
        vec4<f32>(m[1].xyz, 0.0),
        vec4<f32>(m[2].xyz, 0.0),
        vec4<f32>(0.0, 0.0, 0.0, 1.0),
    );
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let world_pos = model * vec4<f32>(in.position, 1.0);
    out.world_pos = world_pos.xyz;
    out.clip_pos = camera.view_proj * world_pos;

    let normal_matrix = calculate_normal_matrix(model);
    out.normal = normalize((normal_matrix * vec4<f32>(in.normal, 0.0)).xyz);
    out.texcoord = in.texcoord;
    out.color = in.color;
    return out;
}

// PBR Functions
fn fresnel_schlick(cos_theta: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cos_theta, 0.0, 1.0), 5.0);
}

fn distribution_ggx(n: vec3<f32>, h: vec3<f32>, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let n_dot_h = max(dot(n, h), 0.0);
    let n_dot_h2 = n_dot_h * n_dot_h;

    let nom = a2;
    var denom = (n_dot_h2 * (a2 - 1.0) + 1.0);
    denom = 3.14159265359 * denom * denom;
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let n = normalize(in.normal);
    let v = normalize(camera.view_position - in.world_pos);

    // Material properties
    let albedo = material.albedo * in.color;
    let metallic = material.metallic;
    let roughness = material.roughness;
    let ao = material.ao;

    // Calculate reflectance at normal incidence
    var f0 = vec3<f32>(0.04);
    f0 = mix(f0, albedo.rgb, metallic);

    // Light calculation
    let light_dir = normalize(light.position - in.world_pos);
    let h = normalize(v + light_dir);

    let distance = length(light.position - in.world_pos);
    let attenuation = light.intensity / (distance * distance);
    let radiance = light.color * attenuation;

    // Cook-Torrance BRDF
    let ndf = distribution_ggx(n, h, roughness);
    let geo = geometry_smith(n, v, light_dir, roughness);
    let fresnel = fresnel_schlick(max(dot(h, v), 0.0), f0);

    let numerator = ndf * geo * fresnel;
    let denominator = 4.0 * max(dot(n, v), 0.0) * max(dot(n, light_dir), 0.0) + 0.0001;
    let specular = numerator / denominator;

    // Energy conservation
    let ks = fresnel;
    var kd = vec3<f32>(1.0) - ks;
    kd *= 1.0 - metallic;

    let n_dot_l = max(dot(n, light_dir), 0.0);
    let lo = (kd * albedo.rgb / 3.14159265359 + specular) * radiance * n_dot_l;

    // Ambient
    let ambient = vec3<f32>(0.03) * albedo.rgb * ao;
    var color = ambient + lo;

    // HDR tonemapping
    color = color / (color + vec3<f32>(1.0));

    // Gamma correction
    color = pow(color, vec3<f32>(1.0 / 2.2));

    return vec4<f32>(color, albedo.a);
}