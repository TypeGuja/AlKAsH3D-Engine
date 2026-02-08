#version 450 core

in vec3 vWorldPos;
in vec3 vNormal;
in vec2 vTexCoord;

uniform vec3 uCamPos;
uniform bool uUseTexture;
uniform sampler2D uAlbedo;

out vec4 FragColor;

void main()
{
    // Нормализуем нормаль
    vec3 norm = normalize(vNormal);

    // Направление взгляда
    vec3 viewDir = normalize(uCamPos - vWorldPos);

    // Простой направленный свет (по умолчанию)
    vec3 lightDir = normalize(vec3(-1.0, -1.0, -1.0));

    // Диффузное освещение
    float diff = max(dot(norm, -lightDir), 0.0);

    // Базовый цвет (белый)
    vec3 baseColor = uUseTexture ? texture(uAlbedo, vTexCoord).rgb : vec3(1.0);

    // Простая модель освещения: ambent + diffuse
    vec3 ambient = baseColor * 0.3;
    vec3 diffuse = baseColor * diff * 0.7;

    vec3 result = ambient + diffuse;
    FragColor = vec4(result, 1.0);
}