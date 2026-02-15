#version 450 core
#define MAX_LIGHTS 8

struct Light {
    int   type;          // -1.txt = none, 0 = directional, 1.txt = point, 2 = spot
    vec3  color;
    float intensity;
    vec3  direction;     // directional
    vec3  position;      // point/spot
    float radius;        // point only
    vec3  spotDir;       // spot only
    float innerCutoff;
    float outerCutoff;
};

uniform Light lights[MAX_LIGHTS];
uniform vec3 uCamPos;

uniform bool uUseTexture;          // true → берём texture, false → белый
uniform sampler2D uAlbedo;         // (может быть не привязан, но uniform нужен)

in vec3 vWorldPos;
in vec3 vNormal;
in vec2 vTexCoord;

layout(location = 0) out vec4 fragColor;

vec3 calcLight(Light L, vec3 N, vec3 V, vec3 P)
{
    vec3 Ldir;
    float attenuation = 1.0;

    if (L.type == 0) {               // directional
        Ldir = normalize(-L.direction);
    } else if (L.type == 1) {        // point
        Ldir = L.position - P;
        float dist = length(Ldir);
        Ldir = normalize(Ldir);
        attenuation = 1.0 / (dist * dist);
    } else if (L.type == 2) {        // spot
        Ldir = L.position - P;
        float dist = length(Ldir);
        Ldir = normalize(Ldir);
        float theta = dot(Ldir, normalize(-L.spotDir));
        float epsilon = L.innerCutoff - L.outerCutoff;
        float intensity = clamp((theta - L.outerCutoff) / epsilon, 0.0, 1.0);
        attenuation = intensity / (dist * dist);
    } else {
        return vec3(0.0);
    }

    // Diffuse
    float diff = max(dot(N, Ldir), 0.0);
    vec3 diffuse = diff * L.color * L.intensity;

    // Specular (Blinn‑Phong)
    vec3 H = normalize(Ldir + V);
    float spec = pow(max(dot(N, H), 0.0), 32.0);
    vec3 specular = spec * L.color * L.intensity;

    return attenuation * (diffuse + specular);
}

void main()
{
    vec3 N = normalize(vNormal);
    vec3 V = normalize(uCamPos - vWorldPos);
    vec3 albedo;

    // Если мы хотим использовать реальную текстуру – ставим uUseTexture = true
    // (по умолчанию в ForwardRenderer мы ставим false → белый цвет).
    if (uUseTexture) {
        albedo = texture(uAlbedo, vTexCoord).rgb;
    } else {
        albedo = vec3(1.0);          // fallback‑цвет – полностью белый
    }

    vec3 result = vec3(0.0);
    for (int i = 0; i < MAX_LIGHTS; ++i) {
        if (lights[i].type == -1) break;
        result += calcLight(lights[i], N, V, vWorldPos);
    }

    fragColor = vec4(result * albedo, 1.0);
}