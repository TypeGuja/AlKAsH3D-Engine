// MSAA задаётся из Rust (`#define MSAA 1`, см. compile_*_shaders): при MSAA
// depth-таргет многосэмпловый и SRV на него обязан быть Texture2DMS.
#ifdef MSAA
Texture2DMS<float> DepthBuffer : register(t0);
#else
Texture2D<float> DepthBuffer : register(t0);
#endif
SamplerState PointSampler : register(s0);

cbuffer SSAOParams : register(b0) {
    float4x4 viewProj;
    float4x4 invViewProj;
    float3   cameraPos;
    float    radius;
    float    bias;
    float    strength;
    float2   _padding0;
};

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

static const int NUM_SAMPLES = 12;

#ifdef MSAA
float LoadDepth(int2 coord) { return DepthBuffer.Load(coord, 0).r; }
void GetDepthDims(out uint w, out uint h) { uint s; DepthBuffer.GetDimensions(w, h, s); }
#else
float LoadDepth(int2 coord) { return DepthBuffer.Load(int3(coord, 0)).r; }
void GetDepthDims(out uint w, out uint h) { DepthBuffer.GetDimensions(w, h); }
#endif

float hash1(float2 p, float2 seed) {
    return frac(sin(dot(p + seed, float2(12.9898, 78.233))) * 43758.5453);
}

// Та же формула восстановления мировой позиции по глубине, что уже
// проверена в volumetric (compile_volumetric_shaders) — переиспользуем
// намеренно один и тот же, уже подтверждённый вживую вывод, а не
// выводим параллельную view-space версию с риском перепутать знак/
// handedness там, где я не могу быстро увидеть результат глазами.
float3 reconstructWorldPos(float2 uv, float depth) {
    float ndcX = uv.x * 2.0 - 1.0;
    float ndcY = 1.0 - uv.y * 2.0;
    float4 clip = float4(ndcX, ndcY, depth, 1.0);
    float4 world = mul(invViewProj, clip);
    return world.xyz / world.w;
}

float4 main(PS_INPUT input) : SV_TARGET {
    uint depthW, depthH;
    GetDepthDims(depthW, depthH);
    int2 depthCoord = int2(input.uv * float2(depthW, depthH));
    float depth = LoadDepth(depthCoord);

    // depth == 1.0 — небо/пустота, окклюзии в принципе нет.
    if (depth >= 0.9999) {
        return float4(1.0, 1.0, 1.0, 1.0);
    }

    float3 worldPos = reconstructWorldPos(input.uv, depth);

    // Нормаль из экранных производных мировой позиции — заменяет
    // normal G-buffer, которого у этого (forward) рендерера нет.
    // Знак выбирается так, чтобы нормаль ГАРАНТИРОВАННО смотрела на
    // камеру, независимо от знакового соглашения ddx/ddy — защита
    // от переворота окклюзии "наизнанку" на части экрана.
    float3 normal = normalize(cross(ddx(worldPos), ddy(worldPos)));
    if (dot(normal, cameraPos - worldPos) < 0.0) {
        normal = -normal;
    }

    float3 up = (abs(normal.y) < 0.99) ? float3(0.0, 1.0, 0.0) : float3(1.0, 0.0, 0.0);
    float3 tangent = normalize(cross(up, normal));
    float3 bitangent = cross(normal, tangent);

    float occlusion = 0.0;
    [unroll]
    for (int i = 0; i < NUM_SAMPLES; i++) {
        // Косинус-взвешенное распределение по полусфере в ЛОКАЛЬНОМ
        // (относительно нормали) пространстве — u1/u2 варьируются и
        // по пикселю (input.uv), и по индексу сэмпла (seed), поэтому
        // соседние пиксели получают РАЗНЫЕ паттерны сэмплов — тот же
        // приём, что убирает "полосы" у volumetric raymarch (см. его
        // jitter), только здесь заменяет собой отдельный blur-проход
        // целиком, а не только маскирует шаг вдоль луча.
        float2 seed = float2(float(i) * 0.13, float(i) * 0.71);
        float u1 = hash1(input.uv, seed);
        float u2 = hash1(input.uv, seed + float2(0.37, 0.91));
        float r = sqrt(u1);
        float theta = 6.28318530718 * u2;
        float lx = r * cos(theta);
        float ly = r * sin(theta);
        float lz = sqrt(max(0.0, 1.0 - u1));
        // Сэмплы ближе к центру полусферы весят больше — стандартный
        // приём SSAO-ядер (кластеризация сэмплов у начала координат),
        // даёт более выраженную окклюзию у самых близких преград.
        float t = (float(i) + 0.5) / float(NUM_SAMPLES);
        float scale = lerp(0.1, 1.0, t * t);
        float3 localDir = float3(lx, ly, lz) * scale;
        float3 worldDir = tangent * localDir.x + bitangent * localDir.y + normal * localDir.z;

        float3 samplePos = worldPos + worldDir * radius;

        float4 clip = mul(viewProj, float4(samplePos, 1.0));
        if (clip.w <= 0.0001) continue;
        float3 ndc = clip.xyz / clip.w;
        float2 sampleUV = float2(ndc.x * 0.5 + 0.5, 1.0 - (ndc.y * 0.5 + 0.5));
        if (sampleUV.x < 0.0 || sampleUV.x > 1.0 || sampleUV.y < 0.0 || sampleUV.y > 1.0) continue;

        int2 sampleDepthCoord = int2(sampleUV * float2(depthW, depthH));
        float sceneDepth = LoadDepth(sampleDepthCoord);
        if (sceneDepth >= 0.9999) continue; // небо в этом направлении — окклюдировать нечем

        float3 sceneWorldPos = reconstructWorldPos(sampleUV, sceneDepth);

        float distSample = length(samplePos - cameraPos);
        float distScene = length(sceneWorldPos - cameraPos);

        // Затухание вклада при большой разнице глубин — стандартный
        // приём SSAO (Crysis-style range check), убирает широкие
        // "ореолы" окклюзии вокруг тонких/далёких объектов.
        float rangeCheck = saturate(radius / max(abs(distSample - distScene), 0.0001));
        occlusion += ((distScene <= distSample - bias) ? 1.0 : 0.0) * rangeCheck;
    }
    occlusion = occlusion / float(NUM_SAMPLES);

    float ao = 1.0 - saturate(occlusion * strength);
    return float4(ao, ao, ao, 1.0);
}
