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

// ДОБАВЛЕНО (отражения лучами): освещение точки попадания луча — общее
// для лучей проб и отражённых лучей. false — попали в изнанку (луч
// изнутри геометрии), освещения нет.
bool ShadeHit(uint instanceId, uint prim, float2 bary, float3x3 worldToObject, float3 origin, float3 dir, float t, out float3 outRadiance) {
    outRadiance = float3(0.0, 0.0, 0.0);
    RtInstance inst = Instances[instanceId];
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
    float3 n = normalize(mul(nObj, worldToObject));

    // Попали в изнанку: проба смотрит изнутри геометрии. Короткое
    // отрицательное расстояние — метка для отключения пробы.
    if (dot(n, dir) > 0.0) {
        return false;
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
    outRadiance = min(radiance, 60000.0);
    return true;
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
    float3 radiance;
    if (!ShadeHit(q.CommittedInstanceID(), q.CommittedPrimitiveIndex(), q.CommittedTriangleBarycentrics(), (float3x3)q.CommittedWorldToObject3x4(), origin, dir, t, radiance)) {
        // Попали в изнанку: проба смотрит изнутри геометрии. Короткое
        // отрицательное расстояние — метка для отключения пробы.
        RayData[uint2(rayIndex, probeIndex)] = float4(0.0, 0.0, 0.0, -t * 0.2);
        return;
    }
    RayData[uint2(rayIndex, probeIndex)] = float4(radiance, t);
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

// ДОБАВЛЕНО (отражения лучами): по G-буферу основного прохода из каждого
// пикселя — один отражённый луч по распределению микрограней GGX (каждый
// кадр другой, TAA усредняет). Яркость в точке попадания — как у лучей
// проб (прямой свет с теневыми лучами + пробы). Очень шершавое — из проб
// без луча. Результат: цвет + вес_отражения * яркость отражения.
// ИЗМЕНЕНО (25 FPS на 1080p — лучи из каждого пикселя ~6.7 мс): луч один
// на блок 2x2 (TraceReflections, половинное разрешение), пиксель в блоке
// меняется каждый кадр — за 4 кадра TAA собирает полное. ResolveReflections
// на полном разрешении берёт лучи соседних блоков с весом по расстоянию,
// нормали и шероховатости (через край предмета отражение не протекает).
Texture2D<float4> SpecTex : register(t6);
Texture2D<float4> NormalTex : register(t7);
Texture2D<float4> ColorTex : register(t8);
// rgb — отражённая яркость, a — расстояние от камеры (< 0 — луча нет)
Texture2D<float4> ReflHalfTex : register(t9);
RWTexture2D<float4> ReflOut : register(u3);
RWTexture2D<float4> ReflHalf : register(u4);

cbuffer ReflConstants : register(b1) {
    float4x4 reflInvViewProj;
    float3 reflCamPos;
    uint reflFrame;
    float2 reflSize;
    float reflMaxRoughness;
    float _reflPad;
};

float Hash1(uint3 v) {
    uint h = v.x * 1973u + v.y * 9277u + v.z * 26699u;
    h = (h ^ 61u) ^ (h >> 16); h *= 9u; h ^= h >> 4; h *= 0x27d4eb2du; h ^= h >> 15;
    return (h & 0xFFFFFFu) / 16777216.0;
}


/// Пиксель блока 2x2, из которого в этом кадре идёт луч.
int2 ReflTracedPixel(int2 block) {
    static const int2 offs[4] = { int2(0, 0), int2(1, 1), int2(1, 0), int2(0, 1) };
    return min(block * 2 + offs[reflFrame & 3u], int2(reflSize) - 1);
}

/// Точка, нормаль (к камере) и направление к камере пикселя по G-буферу.
void ReflSurface(int2 p, float4 nd, out float3 P, out float3 N, out float3 V) {
    float2 uv = (float2(p) + 0.5) / reflSize;
    float4 farP = mul(reflInvViewProj, float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 1.0, 1.0));
    float3 viewDir = normalize(farP.xyz / farP.w - reflCamPos);
    // в альфе нормали — расстояние от камеры (MSAA-край — среднее, это
    // смесь двух поверхностей, сдвиг от неё к камере безопасен)
    P = reflCamPos + viewDir * nd.a;
    N = normalize(nd.rgb * 2.0 - 1.0);
    V = -viewDir;
    if (dot(N, V) < 0.0) {
        N = -N;
    }
}

/// Шершавое: отражение целиком из проб (малая дисперсия, луч не нужен).
float3 ReflFromProbes(float3 P, float3 N, float3 V, float rough) {
    float3 R = reflect(-V, N);
    float4 probesN = DDGISampleIrradiance(P, N, V, volume, IrradianceSrv, DistanceSrv, LinearClamp);
    float4 probesR = DDGISampleIrradiance(P, R, V, volume, IrradianceSrv, DistanceSrv, LinearClamp);
    float3 a = probesR.a > 0.5 ? probesR.rgb : skyRadiance.rgb * skyRadiance.a;
    float3 b = probesN.a > 0.5 ? probesN.rgb : a;
    return lerp(a, b, rough);
}

[numthreads(8, 8, 1)]
void TraceReflections(uint3 id : SV_DispatchThreadID) {
    int2 block = int2(id.xy);
    if (any(block * 2 >= int2(reflSize))) {
        return;
    }
    int2 p = ReflTracedPixel(block);
    float4 nd = NormalTex.Load(int3(p, 0));
    float4 sp = SpecTex.Load(int3(p, 0));
    float rough = saturate(sp.a);
    // небо, светящееся, стекло, шершавое — луча нет (шершавое сборка
    // берёт из проб сама, на полном разрешении)
    if (nd.a <= 0.0 || max(sp.r, max(sp.g, sp.b)) <= 0.0 || rough > reflMaxRoughness) {
        ReflHalf[block] = float4(0.0, 0.0, 0.0, -1.0);
        return;
    }
    float3 P, N, V;
    ReflSurface(p, nd, P, N, V);
    // ДОБАВЛЕНО (окно во двор: почти зеркальные тёмные окна дома напротив —
    // луч из каждого пикселя на всю дальность сцены): за объёмом проб луча
    // нет, отражение — из проб/неба (сборка). Блик фонаря на стекле считает
    // основной проход (зеркальная часть прямого света), не отражённый луч.
    float3 cell = (P - volume.origin) / volume.spacing;
    if (any(cell < 0.0) || any(cell > float3(volume.counts) - 1.0)) {
        ReflHalf[block] = float4(0.0, 0.0, 0.0, -1.0);
        return;
    }

    // полувектор по GGX (выборка по D(h) cos)
    float alpha = max(rough * rough, 0.002);
    float u1 = Hash1(uint3(p, reflFrame));
    float u2 = Hash1(uint3(p.yx + 7, reflFrame * 3u + 1u));
    float phi = 2.0 * PI * u2;
    float cosT = sqrt((1.0 - u1) / (1.0 + (alpha * alpha - 1.0) * u1));
    float sinT = sqrt(saturate(1.0 - cosT * cosT));
    float3 up = abs(N.y) < 0.999 ? float3(0.0, 1.0, 0.0) : float3(1.0, 0.0, 0.0);
    float3 T = normalize(cross(up, N));
    float3 B = cross(N, T);
    float3 H = normalize(T * (sinT * cos(phi)) + B * (sinT * sin(phi)) + N * cosT);
    float3 R = reflect(-V, H);
    if (dot(R, N) <= 0.0) {
        R = reflect(-V, N);
    }

    float3 L = float3(0.0, 0.0, 0.0);
    RayDesc ray;
    ray.Origin = P + N * 0.01;
    ray.Direction = R;
    ray.TMin = 0.002;
    ray.TMax = maxDistance;
    RayQuery<RAY_FLAG_CULL_NON_OPAQUE | RAY_FLAG_SKIP_PROCEDURAL_PRIMITIVES> q;
    q.TraceRayInline(Scene, RAY_FLAG_NONE, 0xFF, ray);
    q.Proceed();
    if (q.CommittedStatus() != COMMITTED_TRIANGLE_HIT) {
        L = skyRadiance.rgb * skyRadiance.a;
    } else {
        float3 hitL;
        if (ShadeHit(q.CommittedInstanceID(), q.CommittedPrimitiveIndex(), q.CommittedTriangleBarycentrics(), (float3x3)q.CommittedWorldToObject3x4(), ray.Origin, R, q.CommittedRayT(), hitL)) {
            L = hitL;
        }
    }
    if (!DDGIFinite3(L)) {
        L = float3(0.0, 0.0, 0.0);
    }
    // Один луч на пиксель — шум. Смешиваем с оценкой того же интеграла по
    // пробам (малая дисперсия, но размыто): чем шире лепесток GGX, тем
    // больше доля проб — как у Lumen. И гасим «светлячков» — лучи,
    // попавшие в редкое яркое пятно, сильнее всего шумят.
    float4 probesN = DDGISampleIrradiance(P, N, V, volume, IrradianceSrv, DistanceSrv, LinearClamp);
    float4 probesR = DDGISampleIrradiance(P, R, V, volume, IrradianceSrv, DistanceSrv, LinearClamp);
    float3 pr = probesR.a > 0.5 ? probesR.rgb : (probesN.a > 0.5 ? probesN.rgb : L);
    float lumP = dot(pr, float3(0.2126, 0.7152, 0.0722));
    float lumL = dot(L, float3(0.2126, 0.7152, 0.0722));
    float cap = lumP * 6.0 + 0.05;
    if (lumL > cap) {
        L *= cap / lumL;
    }
    L = lerp(L, pr, smoothstep(0.12, reflMaxRoughness, rough));
    ReflHalf[block] = float4(min(L, 60000.0), nd.a);
}

[numthreads(8, 8, 1)]
void ResolveReflections(uint3 id : SV_DispatchThreadID) {
    int2 p = int2(id.xy);
    if (p.x >= (int)reflSize.x || p.y >= (int)reflSize.y) {
        return;
    }
    float4 color = ColorTex.Load(int3(p, 0));
    float4 nd = NormalTex.Load(int3(p, 0));
    float4 sp = SpecTex.Load(int3(p, 0));
    // небо, светящееся, стекло — без отражённого окружения
    if (nd.a <= 0.0 || max(sp.r, max(sp.g, sp.b)) <= 0.0) {
        ReflOut[p] = color;
        return;
    }
    float3 P, N, V;
    ReflSurface(p, nd, P, N, V);
    float rough = saturate(sp.a);
    float3 L;
    if (rough > reflMaxRoughness) {
        L = ReflFromProbes(P, N, V, rough);
    } else {
        // лучи блоков 3x3 вокруг: вес — гаусс по расстоянию до пикселя,
        // из которого шёл луч, и похожесть поверхности
        float3 Nraw = normalize(nd.rgb * 2.0 - 1.0);
        int2 hc = p >> 1;
        int2 hmax = (int2(reflSize) + 1) / 2 - 1;
        float3 sum = float3(0.0, 0.0, 0.0);
        float wsum = 0.0;
        [unroll] for (int dy = -1; dy <= 1; dy++) {
            [unroll] for (int dx = -1; dx <= 1; dx++) {
                int2 b = clamp(hc + int2(dx, dy), int2(0, 0), hmax);
                float4 t = ReflHalfTex.Load(int3(b, 0));
                if (t.a < 0.0) {
                    continue;
                }
                int2 q = ReflTracedPixel(b);
                float4 ndq = NormalTex.Load(int3(q, 0));
                float rq = saturate(SpecTex.Load(int3(q, 0)).a);
                float2 d = float2(q - p);
                float w = exp(-dot(d, d) * 0.25);
                w *= pow(saturate(abs(dot(Nraw, normalize(ndq.rgb * 2.0 - 1.0)))), 16.0);
                w *= exp(-abs(t.a - nd.a) / (0.02 * nd.a + 0.01));
                w *= exp(-abs(rq - rough) * 10.0);
                sum += t.rgb * w;
                wsum += w;
            }
        }
        // рядом нет похожей поверхности с лучом (тонкая деталь) — из проб
        L = wsum > 1e-3 ? sum / wsum : ReflFromProbes(P, N, V, rough);
    }
    ReflOut[p] = float4(color.rgb + sp.rgb * L, color.a);
}
