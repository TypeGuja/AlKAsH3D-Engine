#version 450 core
in vec3 vNormal;
in vec2 vTexCoord;
in vec3 vWorldPos;

layout(location = 0) out vec4 fragColor;

uniform sampler2D uAlbedo;      // текстура
uniform vec3 uLightPos = vec3(5.0, 5.0, 5.0);
uniform vec3 uCamPos;

void main()
{
    vec3 albedo = texture(uAlbedo, vTexCoord).rgb;
    vec3 normal = normalize(vNormal);
    vec3 lightDir = normalize(uLightPos - vWorldPos);
    float diff = max(dot(normal, lightDir), 0.0);
    vec3 diffuse = diff * albedo;

    // простейший Blinn‑Phong specular
    vec3 viewDir = normalize(uCamPos - vWorldPos);
    vec3 halfDir = normalize(lightDir + viewDir);
    float spec = pow(max(dot(normal, halfDir), 32.0);
    vec3 specular = vec3(0.3) * spec;

    vec3 color = diffuse + specular;
    fragColor = vec4(color, 1.0);
}
