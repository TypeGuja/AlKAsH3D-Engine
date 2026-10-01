// MSAA задаётся из Rust (`#define MSAA 1`, см. compile_*_shaders): при MSAA
// depth-таргет многосэмпловый и SRV на него обязан быть Texture2DMS.
#ifdef MSAA
Texture2DMS<float> DepthBuffer : register(t0);
#else
Texture2D<float> DepthBuffer : register(t0);
#endif
Texture2D ShadowMap : register(t1);
SamplerState PointSampler : register(s0);
SamplerComparisonState ShadowSampler : register(s1);

cbuffer VolumetricParams : register(b0) {
    float4x4 invViewProj;
    float4x4 lightViewProj;
    float3   cameraPos;
    float    intensity;
    float3   lightDir;   // направление, КУДА летит свет (как TransformConstants.light_dir)
    float    _padding0;
    float3   lightColor;
    float    maxDistance; // дальше этого расстояния от камеры raymarch не идёт
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

// 3x3 PCF, идентичный основному пиксельному шейдеру (см.
// SampleShadowPCF в compile_default_shaders) — используем тот же
// приём, чтобы шаги raymarch'а не давали "рваный" резкий край
// между освещённым и затенённым воздухом.
float SampleShadowPCF(float3 shadowCoord) {
    float shadow = 0.0;
    float texelSize = 1.0 / 2048.0; // SHADOW_MAP_RESOLUTION — см. engine/mod.rs
    [unroll]
    for (int x = -1; x <= 1; x++) {
        [unroll]
        for (int y = -1; y <= 1; y++) {
            float2 offset = float2(x, y) * texelSize;
            shadow += ShadowMap.SampleCmpLevelZero(ShadowSampler, shadowCoord.xy + offset, shadowCoord.z);
        }
    }
    return shadow / 9.0;
}

static const int NUM_STEPS = 24;

#ifdef MSAA
float LoadDepth(int2 coord) { return DepthBuffer.Load(coord, 0).r; }
void GetDepthDims(out uint w, out uint h) { uint s; DepthBuffer.GetDimensions(w, h, s); }
#else
float LoadDepth(int2 coord) { return DepthBuffer.Load(int3(coord, 0)).r; }
void GetDepthDims(out uint w, out uint h) { DepthBuffer.GetDimensions(w, h); }
#endif

float4 main(PS_INPUT input) : SV_TARGET {
    // LoadDepth адресуется ЦЕЛЫМИ пиксельными координатами
    // исходного (полноразмерного) depth-таргета, не [0,1] UV —
    // GetDepthDims даёт его реальный размер (может отличаться от
    // размера ЭТОГО, half-res, render target'а).
    uint depthW, depthH;
    GetDepthDims(depthW, depthH);
    int2 depthCoord = int2(input.uv * float2(depthW, depthH));
    float depth = LoadDepth(depthCoord);

    // depth == 1.0 (дальняя плоскость очистки, см.
    // create_depth_stencil::clear_value) означает "нет геометрии в
    // этом пикселе — небо/пустота". Raymarch в этом случае идёт до
    // maxDistance вдоль луча вместо до реальной геометрии — иначе
    // god rays никогда бы не были видны на фоне неба, что не
    // соответствует тому, как они выглядят в реальности (свет,
    // рассеянный в воздухе МЕЖДУ камерой и любой преградой,
    // включая "нет преграды вообще").
    float ndcX = input.uv.x * 2.0 - 1.0;
    float ndcY = 1.0 - input.uv.y * 2.0;

    float3 rayEnd;
    if (depth >= 0.9999) {
        // Точка на дальней плоскости отсечения в направлении этого
        // пикселя — используем как временную "конечную точку" луча,
        // затем всё равно ограничиваем маршем через maxDistance
        // ниже.
        float4 farClip = mul(invViewProj, float4(ndcX, ndcY, 1.0, 1.0));
        rayEnd = farClip.xyz / farClip.w;
    } else {
        float4 worldPos = mul(invViewProj, float4(ndcX, ndcY, depth, 1.0));
        rayEnd = worldPos.xyz / worldPos.w;
    }

    float3 rayDir = rayEnd - cameraPos;
    float rayLength = length(rayDir);
    rayDir /= max(rayLength, 0.0001);
    rayLength = min(rayLength, maxDistance);

    float stepSize = rayLength / float(NUM_STEPS);
    // Небольшой случайный сдвиг стартовой точки шага (по
    // экранным координатам, детерминированный — не зависит от
    // кадра) убирает видимые полосы-артефакты (banding) от
    // слишком малого числа шагов, "размазывая" их в шум, который
    // визуально гораздо менее заметен, чем регулярные полосы.
    float jitter = frac(sin(dot(input.uv, float2(12.9898, 78.233))) * 43758.5453);

    float accumulated = 0.0;
    for (int i = 0; i < NUM_STEPS; i++) {
        float t = (float(i) + jitter) * stepSize;
        float3 samplePos = cameraPos + rayDir * t;

        float4 lightSpacePos = mul(lightViewProj, float4(samplePos, 1.0));
        if (lightSpacePos.w > 0.0001) {
            float3 shadowCoord = lightSpacePos.xyz / lightSpacePos.w;
            float2 shadowUV = float2(shadowCoord.x * 0.5 + 0.5, 1.0 - (shadowCoord.y * 0.5 + 0.5));
            if (shadowUV.x >= 0.0 && shadowUV.x <= 1.0 && shadowUV.y >= 0.0 && shadowUV.y <= 1.0 && shadowCoord.z >= 0.0 && shadowCoord.z <= 1.0) {
                accumulated += SampleShadowPCF(float3(shadowUV, shadowCoord.z));
            } else {
                // Вне shadow map (например, очень далеко от камеры,
                // за пределами frustum-fitted ортопроекции) — по
                // умолчанию считаем ОСВЕЩЁННЫМ (тот же safe fallback,
                // что и border-цвет compare-сэмплера в основном
                // пиксельном шейдере), чтобы god rays не обрывались
                // резкой чёрной границей на краю shadow-объёма.
                accumulated += 1.0;
            }
        }
    }
    accumulated /= float(NUM_STEPS);

    // Дополнительно взвешиваем по тому, насколько луч вообще
    // направлен "к камере" от солнца (сильнее god rays видны,
    // когда смотришь примерно НА солнце, а не спиной к нему) —
    // стандартный приём, делающий эффект направленным, а не
    // равномерным туманом.
    float sunFacing = saturate(dot(-rayDir, normalize(lightDir)) * 0.5 + 0.5);

    float3 result = lightColor * accumulated * intensity * (0.3 + 0.7 * sunFacing);
    return float4(result, 1.0);
}
