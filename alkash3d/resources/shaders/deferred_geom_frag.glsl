#version 450 core
in vec3 vWorldPos;
in vec3 vNormal;
in vec2 vTexCoord;

layout(location = 0) out vec3 gPosition;
layout(location = 1) out vec3 gNormal;
layout(location = 2) out vec4 gAlbedoSpec; // rgb = albedo, a = specular

uniform sampler2D uAlbedo;   // если нет – будем использовать белый
uniform sampler2D uSpecular; // если нет – 0

void main()
{
    gPosition = vWorldPos;
    gNormal   = normalize(vNormal);

    // Если текстур нет, texture() вернёт чёрный (0). Поэтому
    // делаем fallback к белому/нулевому.
    vec3 albedo = texture(uAlbedo, vTexCoord).rgb;
    float spec = texture(uSpecular, vTexCoord).r;

    // Fallback, если текстурный фреймбуфер пуст (all zeros):
    //   – белый альбедо (1,1,1)
    //   – отсутствие спекуляра (0)
    if (all(equal(albedo, vec3(0.0)))) {
        albedo = vec3(1.0);
    }
    if (spec == 0.0) {
        spec = 0.0; // уже 0, оставляем как есть (но можно явно задать)
    }

    gAlbedoSpec = vec4(albedo, spec);
}