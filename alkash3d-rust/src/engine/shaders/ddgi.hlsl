// ДОБАВЛЕНО (непрямой свет лучами, DDGI): проходы обновления проб —
// компилируются DXC (SM 6.5, inline ray tracing). Перед этим файлом
// движок подставляет ddgi_common.hlsl.
//
//  TraceRays        — из каждой пробы raysPerProbe лучей по сфере Фибоначчи
//                     (каждый кадр со случайным поворотом). В точке попадания:
//                     прямой свет (солнце и фонари с теневыми лучами),
//                     излучение поверхности и освещённость из самих проб
//                     (прошлый кадр) — так набираются многократные отскоки.
//  UpdateIrradiance — косинусно-взвешенное среднее лучей по направлениям
//                     плитки, смешивание с прошлым (гистерезис).
//  UpdateDistance   — то же для расстояния и его квадрата (видимость).
//  BorderIrradiance / BorderDistance — рамки плиток.
//  ClearAtlases     — начальное состояние атласов.

struct GPULight {
    float4 position;
    float4 color;
    float4 direction;
    float4 params;
};

// Материал инстанса для лучей: средний цвет (вершины * текстура * tint),
// излучение, индексы буферов вершин/индексов в bindless-таблице.
struct RtInstance {
    float3 albedo;
    uint vertexBuffer;
    float3 emissive;
    uint indexBuffer;  // 0xFFFFFFFF — меш без индексов
    float metallic;
    uint3 _pad;
};

cbuffer GIConstants : register(b0) {
    DDGIVolume volume;
    float4x4 rayRotation;  // поворот сферы Фибоначчи на этот кадр
    float4 sunDir;         // xyz — куда светит солнце (lightDir), w — включено
    float4 sunColor;       // rgb * a — как lightColor
    float4 skyRadiance;    // rgb * a — как ambientColor
    uint raysPerProbe;
    uint lightCount;
    float hysteresis;
    float maxDistance;
    // Пробы, обновляемые в этом кадре: probeOffset.. (по кругу), probeBudget штук.
    // Строка данных лучей и номер группы — номер внутри этой порции.
    uint probeOffset;
    uint probeBudget;
    uint2 _pad2;
};

uint ProbeOfSlot(uint slot) {
    uint total = volume.counts.x * volume.counts.y * volume.counts.z;
    return (slot + probeOffset) % total;
}

RaytracingAccelerationStructure Scene : register(t0);
StructuredBuffer<RtInstance> Instances : register(t1);
StructuredBuffer<GPULight> Lights : register(t2);
Texture2D<float4> IrradianceSrv : register(t3);
Texture2D<float4> DistanceSrv : register(t4);
Texture2D<float4> RayDataSrv : register(t5);
RWTexture2D<float4> RayData : register(u0);
RWTexture2D<float4> Irradiance : register(u1);
RWTexture2D<float4> Distance : register(u2);
ByteAddressBuffer Buffers[] : register(t0, space1);
SamplerState LinearClamp : register(s0);

static const uint VERTEX_STRIDE = 68;  // Vertex: position f4, normal f3, color f4, uv f2, tangent f4
static const float PI = 3.14159265;

float3 SphericalFibonacci(float i, float n) {
    const float PHI = 1.6180339887;
    float phi = 2.0 * PI * frac(i * (PHI - 1.0));
    float cosTheta = 1.0 - (2.0 * i + 1.0) / n;
    float sinTheta = sqrt(saturate(1.0 - cosTheta * cosTheta));
    return float3(cos(phi) * sinTheta, sin(phi) * sinTheta, cosTheta);
}

float3 RayDirection(uint i) {
    return normalize(mul((float3x3)rayRotation, SphericalFibonacci((float)i, (float)raysPerProbe)));
}

bool Visible(float3 from, float3 dir, float maxT) {
    RayDesc ray;
    ray.Origin = from;
    ray.Direction = dir;
    ray.TMin = 0.0;
    ray.TMax = maxT;
    RayQuery<RAY_FLAG_CULL_NON_OPAQUE | RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES | RAY_FLAG_ACCEPT_FIRST_HIT_AND_END_SEARCH> q;
    q.TraceRayInline(Scene, RAY_FLAG_NONE, 0xFF, ray);
    q.Proceed();
    return q.CommittedStatus() != COMMITTED_TRIANGLE_HIT;
}

// Падающий свет фонаря на единицу NdotL — та же формула, что
// ComputePointLightContribution в main_ps.hlsl, только тень — лучом.
float3 LightIncoming(GPULight light, float3 P, float3 n, out float3 toL, out float dist) {
    float type = light.position.w;
    float attenuation;
    if (type > 1.5) {
        toL = normalize(-light.direction.xyz);
        dist = 1e4;
        attenuation = 1.0;
    } else {
        float3 v = light.position.xyz - P;
        dist = length(v);
        toL = dist > 0.0001 ? v / dist : float3(0.0, 1.0, 0.0);
        float range = max(light.direction.w, 0.001);
        float ratio = saturate(dist / range);
        float window = 1.0 - ratio * ratio;
        attenuation = window * window / max(dist * dist, 0.01);
        if (type > 0.5) {
            float cosAngle = dot(normalize(light.direction.xyz), -toL);
            float cosOuter = cos(max(light.params.x, 0.001));
            float cosInner = cos(max(min(light.params.z, light.params.x), 0.0));
            attenuation *= smoothstep(cosOuter, max(cosInner, cosOuter + 0.0001), cosAngle);
        }
    }
    return light.color.rgb * light.color.w * attenuation;
}

uint LoadIndex(RtInstance inst, uint prim, uint k) {
    uint i = prim * 3 + k;
    return inst.indexBuffer == 0xFFFFFFFF ? i : Buffers[NonUniformResourceIndex(inst.indexBuffer)].Load(i * 4);
}

[numthreads(32, 1, 1)]
void TraceRays(uint3 id : SV_DispatchThreadID) {
    uint rayIndex = id.x;
    uint probeIndex = id.y;
    if (rayIndex >= raysPerProbe || probeIndex >= probeBudget) {
        return;
    }
    int3 probe = DDGIProbeCoords(ProbeOfSlot(probeIndex), volume);
    float3 origin = DDGIProbePosition(probe, volume);
    float3 dir = RayDirection(rayIndex);

    RayDesc ray;
    ray.Origin = origin;
    ray.Direction = dir;
    ray.TMin = 0.0;
    ray.TMax = maxDistance;
    RayQuery<RAY_FLAG_CULL_NON_OPAQUE | RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES> q;
    q.TraceRayInline(Scene, RAY_FLAG_NONE, 0xFF, ray);
    q.Proceed();

    if (q.CommittedStatus() != COMMITTED_TRIANGLE_HIT) {
        RayData[uint2(rayIndex, probeIndex)] = float4(skyRadiance.rgb * skyRadiance.a, 1e27);
        return;
    }

    float t = q.CommittedRayT();
    RtInstance inst = Instances[q.CommittedInstanceID()];
    uint prim = q.CommittedPrimitiveIndex();
    float2 bary = q.CommittedTriangleBarycentrics();
    ByteAddressBuffer vb = Buffers[NonUniformResourceIndex(inst.vertexBuffer)];
    uint i0 = LoadIndex(inst, prim, 0), i1 = LoadIndex(inst, prim, 1), i2 = LoadIndex(inst, prim, 2);
    // Нормаль вершин (сторона "наружу" по данным модели) — интерполяция.
    float3 n0 = asfloat(vb.Load3(i0 * VERTEX_STRIDE + 16));
    float3 n1 = asfloat(vb.Load3(i1 * VERTEX_STRIDE + 16));
    float3 n2 = asfloat(vb.Load3(i2 * VERTEX_STRIDE + 16));
    float3 nObj = n0 * (1.0 - bary.x - bary.y) + n1 * bary.x + n2 * bary.y;
    if (dot(nObj, nObj) < 1e-12) {
        float3 p0 = asfloat(vb.Load3(i0 * VERTEX_STRIDE));
        float3 p1 = asfloat(vb.Load3(i1 * VERTEX_STRIDE));
        float3 p2 = asfloat(vb.Load3(i2 * VERTEX_STRIDE));
        nObj = cross(p1 - p0, p2 - p0);
    }
    // Нормаль в мир — обратно-транспонированной матрицей (через WorldToObject).
    float3 n = normalize(mul(nObj, (float3x3)q.CommittedWorldToObject3x4()));

    // Попали в изнанку: проба смотрит изнутри геометрии. Короткое
    // отрицательное расстояние — метка для отключения пробы.
    if (dot(n, dir) > 0.0) {
        RayData[uint2(rayIndex, probeIndex)] = float4(0.0, 0.0, 0.0, -t * 0.2);
        return;
    }

    float3 P = origin + dir * t;
    float3 Poff = P + n * (volume.spacing * 0.01);
    float3 diffuse = inst.albedo * (1.0 - inst.metallic);
    float3 radiance = inst.emissive;

    // Солнце.
    if (sunDir.w > 0.5 && sunColor.a > 0.0) {
        float3 toSun = normalize(-sunDir.xyz);
        float ndl = dot(n, toSun);
        if (ndl > 0.0 && Visible(Poff, toSun, 1e4)) {
            radiance += diffuse * sunColor.rgb * sunColor.a * ndl;
        }
    }
    // Фонари.
    for (uint li = 0; li < lightCount; li++) {
        GPULight light = Lights[li];
        float3 toL;
        float d;
        float3 incoming = LightIncoming(light, P, n, toL, d);
        float ndl = dot(n, toL);
        if (ndl <= 0.0 || max(incoming.r, max(incoming.g, incoming.b)) <= 0.0) {
            continue;
        }
        if (Visible(Poff, toL, max(d - volume.spacing * 0.02, 0.0))) {
            radiance += diffuse * incoming * ndl;
        }
    }
    // Многократные отскоки: освещённость в точке попадания из проб.
    float4 indirect = DDGISampleIrradiance(P, n, -dir, volume, IrradianceSrv, DistanceSrv, LinearClamp);
    float3 ambient = indirect.a > 0.5 ? indirect.rgb : skyRadiance.rgb * skyRadiance.a;
    radiance += diffuse * ambient;

    if (!DDGIFinite3(radiance)) {
        radiance = float3(0.0, 0.0, 0.0);
    }
    // Предел half-float атласа.
    RayData[uint2(rayIndex, probeIndex)] = float4(min(radiance, 60000.0), t);
}

// Одна группа — одна проба, поток — тексель внутренней части плитки.
[numthreads(DDGI_IRR_N, DDGI_IRR_N, 1)]
void UpdateIrradiance(uint3 gid : SV_GroupID, uint3 tid : SV_GroupThreadID) {
    uint probeIndex = gid.x;
    int3 probe = DDGIProbeCoords(ProbeOfSlot(probeIndex), volume);
    float3 texelDir = DDGIOctDecode(((float2(tid.xy) + 0.5) / DDGI_IRR_N) * 2.0 - 1.0);

    float3 sum = float3(0.0, 0.0, 0.0);
    float wsum = 0.0;
    uint backfaces = 0;
    for (uint r = 0; r < raysPerProbe; r++) {
        float4 ray = RayDataSrv[uint2(r, probeIndex)];
        if (!DDGIFinite3(ray.rgb)) {
            continue;
        }
        if (ray.a < 0.0) {
            backfaces++;
            continue;
        }
        float w = max(0.0, dot(texelDir, RayDirection(r)));
        sum += ray.rgb * w;
        wsum += w;
    }
    uint2 texel = DDGITileOrigin(probe, volume, DDGI_IRR_N) + 1 + tid.xy;
    float4 old = Irradiance[texel];
    float3 fresh = wsum > 1e-6 ? sum / wsum : old.rgb;
    // Проба внутри геометрии: больше четверти лучей в изнанку.
    float active = backfaces * 4 < raysPerProbe ? 1.0 : 0.0;
    // Первый кадр после включения (alpha < 0) — без смешивания с мусором.
    float h = old.a < 0.0 ? 0.0 : hysteresis;
    Irradiance[texel] = float4(lerp(fresh, old.rgb, h), active);
}

[numthreads(DDGI_DIST_N, DDGI_DIST_N, 1)]
void UpdateDistance(uint3 gid : SV_GroupID, uint3 tid : SV_GroupThreadID) {
    uint probeIndex = gid.x;
    int3 probe = DDGIProbeCoords(ProbeOfSlot(probeIndex), volume);
    float3 texelDir = DDGIOctDecode(((float2(tid.xy) + 0.5) / DDGI_DIST_N) * 2.0 - 1.0);

    float2 sum = float2(0.0, 0.0);
    float wsum = 0.0;
    for (uint r = 0; r < raysPerProbe; r++) {
        float d = min(abs(RayDataSrv[uint2(r, probeIndex)].a), maxDistance);
        // Узкий лепесток: расстояние должно быть почти направленным.
        float w = pow(max(0.0, dot(texelDir, RayDirection(r))), 50.0);
        sum += float2(d, d * d) * w;
        wsum += w;
    }
    uint2 texel = DDGITileOrigin(probe, volume, DDGI_DIST_N) + 1 + tid.xy;
    float4 old = Distance[texel];
    float2 fresh = wsum > 1e-6 ? sum / wsum : old.rg;
    float h = old.a < 0.0 ? 0.0 : hysteresis;
    Distance[texel] = float4(lerp(fresh, old.rg, h), 0.0, 1.0);
}

// Рамка: тексель рамки = тексель внутренней части с противоположного края
// октаэдра (развёртка сшивается сама с собой зеркально).
uint2 BorderSource(uint2 local, uint n) {
    uint last = n + 1;
    bool cx = local.x == 0 || local.x == last;
    bool cy = local.y == 0 || local.y == last;
    if (cx && cy) {
        return uint2(local.x == 0 ? n : 1, local.y == 0 ? n : 1);
    }
    if (cy) {
        return uint2(last - local.x, local.y == 0 ? 1 : n);
    }
    return uint2(local.x == 0 ? 1 : n, last - local.y);
}

[numthreads(DDGI_IRR_N + 2, DDGI_IRR_N + 2, 1)]
void BorderIrradiance(uint3 gid : SV_GroupID, uint3 tid : SV_GroupThreadID) {
    uint n = DDGI_IRR_N;
    if (tid.x != 0 && tid.x != n + 1 && tid.y != 0 && tid.y != n + 1) {
        return;
    }
    uint2 origin = DDGITileOrigin(DDGIProbeCoords(ProbeOfSlot(gid.x), volume), volume, n);
    Irradiance[origin + tid.xy] = Irradiance[origin + BorderSource(tid.xy, n)];
}

[numthreads(DDGI_DIST_N + 2, DDGI_DIST_N + 2, 1)]
void BorderDistance(uint3 gid : SV_GroupID, uint3 tid : SV_GroupThreadID) {
    uint n = DDGI_DIST_N;
    if (tid.x != 0 && tid.x != n + 1 && tid.y != 0 && tid.y != n + 1) {
        return;
    }
    uint2 origin = DDGITileOrigin(DDGIProbeCoords(ProbeOfSlot(gid.x), volume), volume, n);
    Distance[origin + tid.xy] = Distance[origin + BorderSource(tid.xy, n)];
}

// Начальное состояние атласов: альфа -1 — "ещё не считалось" (выборка
// такие пробы пропускает, первое обновление пишет без смешивания).
[numthreads(8, 8, 1)]
void ClearAtlases(uint3 id : SV_DispatchThreadID) {
    uint w, h;
    Irradiance.GetDimensions(w, h);
    if (id.x < w && id.y < h) {
        Irradiance[id.xy] = float4(0.0, 0.0, 0.0, -1.0);
    }
    Distance.GetDimensions(w, h);
    if (id.x < w && id.y < h) {
        Distance[id.xy] = float4(0.0, 0.0, 0.0, -1.0);
    }
}
