#version 450 core
#define MAX_LIGHTS 8

struct Light {
    int   type;          // -1.txt = none, 0 = directional, 1.txt = point, 2 = spot
    vec3  color;
    float intensity;
    vec3  direction;    // directional
    vec3  position;     // point/spot
    float radius;       // point only
    vec3  spotDir;      // spot only
    float innerCutoff;
    float outerCutoff;
};

uniform Light lights[MAX_LIGHTS];
uniform int uNumLights;
uniform vec3 uCamPos;

// G‑buffer textures
uniform sampler2D gPosition;
uniform sampler2D gNormal;
uniform sampler2D gAlbedoSpec;

in vec2 vTexCoord;
layout(location = 0) out vec4 fragColor;

vec3 calcLight(Light light, vec3 N, vec3 V, vec3 P)
{
    vec3 L;
    float attenuation = 1.0;

    if (light.type == 0) {            // directional
        L = normalize(-light.direction);
    } else if (light.type == 1) {     // point
        L = light.position - P;
        float dist = length(L);
        L = normalize(L);
        attenuation = 1.0 / (dist * dist);
    } else if (light.type == 2) {     // spot
        L = light.position - P;
        float dist = length(L);
        L = normalize(L);
        float theta = dot(L, normalize(-light.spotDir));
        float epsilon = light.innerCutoff - light.outerCutoff;
        float intensity = clamp((theta - light.outerCutoff) / epsilon, 0.0, 1.0);
        attenuation = intensity / (dist * dist);
    } else {
        return vec3(0.0);
    }

    // diff
    float diff = max(dot(N, L), 0.0);
    vec3 diffuse = diff * light.color * light.intensity;

    // spec (Blinn‑Phong)
    vec3 H = normalize(L + V);
    float spec = pow(max(dot(N, H), 0.0), 32.0);
    vec3 specular = spec * light.color * light.intensity;

    return attenuation * (diffuse + specular);
}

void main()
{
    // Получаем данные из G‑buffer
    vec3 fragPos = texture(gPosition, vTexCoord).rgb;
    vec3 N       = normalize(texture(gNormal, vTexCoord).rgb);
    vec4 albedoSpec = texture(gAlbedoSpec, vTexCoord);
    vec3 albedo = albedoSpec.rgb;
    float specFactor = albedoSpec.a;

    vec3 V = normalize(uCamPos - fragPos);
    vec3 result = vec3(0.0);
    for (int i = 0; i < uNumLights; ++i) {
        if (lights[i].type == -1) break;
        result += calcLight(lights[i], N, V, fragPos);
    }

    // простейшее добавление спекуляра из albedoSpec
    result = result * albedo + specFactor * vec3(0.04);
    fragColor = vec4(result, 1.0);
}
