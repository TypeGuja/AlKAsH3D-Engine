// ДОБАВЛЕНО (непрямой свет лучами, DDGI — Majercik et al., "Dynamic Diffuse
// Global Illumination with Ray-Traced Irradiance Fields", JCGT 2019).
// Общая часть для лучевых проходов (ddgi.hlsl, DXC, SM 6.5) и основного
// шейдера (main_ps.hlsl, FXC, SM 5.0) — только то, что понимают оба.
//
// Сетка проб: probe (x, y, z) стоит в origin + (x, y, z) * spacing. Каждая
// проба хранит в двух атласах октаэдрические карты по всем направлениям:
//  * освещённость — среднее по косинусу входящей яркости (= E / PI, те же
//    единицы, что ambientColor: диффуз поверхности = albedo * это);
//    альфа — 1, если проба рабочая (не внутри геометрии);
//  * расстояния до ближайшей поверхности — среднее и среднее квадрата
//    (проверка видимости по Чебышёву: свет не протекает сквозь стены).
// Плитка пробы — внутренняя часть N x N плюс рамка в 1 тексель (копия
// противоположного края октаэдра — билинейная выборка у края без шва).
// Пробы в атласе: плитка (x + nx * y, z).

#define DDGI_IRR_N 8
#define DDGI_DIST_N 16

struct DDGIVolume {
    float3 origin;
    float spacing;
    uint3 counts;
    uint enabled;
    float2 irrTexel;   // 1 / размер атласа освещённости
    float2 distTexel;  // 1 / размер атласа расстояний
    float normalBias;
    float viewBias;
    float2 _pad;       // x — отладочный показ (1), y — его усиление
};

// Конечное ли число (NaN/Inf из вырожденной геометрии не должны попасть в
// пробы: смешивание разнесло бы их по всей сетке, а bloom — по всему кадру).
bool DDGIFinite3(float3 v) {
    return all(v == v) && all(abs(v) < 1e30);
}

float DDGISignNotZero(float v) {
    return v >= 0.0 ? 1.0 : -1.0;
}

float2 DDGISignNotZero2(float2 v) {
    return float2(DDGISignNotZero(v.x), DDGISignNotZero(v.y));
}

// Направление -> точка октаэдра в [-1, 1]^2.
float2 DDGIOctEncode(float3 n) {
    n /= (abs(n.x) + abs(n.y) + abs(n.z));
    float2 e = n.xy;
    if (n.z < 0.0) {
        e = (1.0 - abs(n.yx)) * DDGISignNotZero2(n.xy);
    }
    return e;
}

float3 DDGIOctDecode(float2 e) {
    float3 n = float3(e.x, e.y, 1.0 - abs(e.x) - abs(e.y));
    if (n.z < 0.0) {
        n.xy = (1.0 - abs(n.yx)) * DDGISignNotZero2(n.xy);
    }
    return normalize(n);
}

uint DDGIProbeIndex(int3 p, DDGIVolume v) {
    return (uint)p.x + v.counts.x * ((uint)p.y + v.counts.y * (uint)p.z);
}

int3 DDGIProbeCoords(uint index, DDGIVolume v) {
    uint x = index % v.counts.x;
    uint y = (index / v.counts.x) % v.counts.y;
    uint z = index / (v.counts.x * v.counts.y);
    return int3(x, y, z);
}

float3 DDGIProbePosition(int3 p, DDGIVolume v) {
    return v.origin + float3(p) * v.spacing;
}

// Левый верхний тексель плитки пробы (вместе с рамкой).
uint2 DDGITileOrigin(int3 p, DDGIVolume v, uint n) {
    return uint2(((uint)p.x + v.counts.x * (uint)p.y) * (n + 2), (uint)p.z * (n + 2));
}

// UV атласа для направления `dir` пробы `p` (плитка с внутренней частью n x n).
float2 DDGIAtlasUV(int3 p, float3 dir, DDGIVolume v, uint n, float2 texel) {
    float2 oct = DDGIOctEncode(dir) * 0.5 + 0.5;
    float2 pixel = float2(DDGITileOrigin(p, v, n)) + 1.0 + oct * n;
    return pixel * texel;
}

// Непрямая освещённость в точке `P` с нормалью `n`, камера/начало луча —
// в направлении `toView` (единичный вектор ОТ точки). Возвращает
// среднюю по косинусу яркость (единицы ambientColor), альфа — была ли
// хоть одна рабочая проба (0 — точка вне сетки или все пробы в стенах).
float4 DDGISampleIrradiance(float3 P, float3 n, float3 toView, DDGIVolume v, Texture2D irr, Texture2D dist, SamplerState s) {
    // Смещение точки от поверхности: к пробам "по нашу сторону" стены.
    float3 biased = P + (n * v.normalBias + toView * v.viewBias) * v.spacing;
    float3 local = (biased - v.origin) / v.spacing;
    int3 maxIdx = int3(v.counts) - 1;
    if (any(local < -0.5) || any(local > float3(maxIdx) + 0.5)) {
        return float4(0.0, 0.0, 0.0, 0.0);
    }
    int3 base = clamp(int3(floor(local)), int3(0, 0, 0), max(maxIdx - 1, int3(0, 0, 0)));
    float3 alpha = saturate(local - float3(base));

    float3 sum = float3(0.0, 0.0, 0.0);
    float wsum = 0.0;
    for (int i = 0; i < 8; i++) {
        int3 off = int3(i & 1, (i >> 1) & 1, (i >> 2) & 1);
        int3 p = min(base + off, maxIdx);
        float3 probePos = DDGIProbePosition(p, v);

        float3 tri = lerp(1.0 - alpha, alpha, float3(off));
        float w = tri.x * tri.y * tri.z;

        // Пробы за поверхностью почти не учитываются (мягко — без щелей).
        float3 toProbe = normalize(probePos - P);
        float wrap = (dot(toProbe, n) + 1.0) * 0.5;
        w *= wrap * wrap + 0.2;

        // Видимость по Чебышёву: если между точкой и пробой поверхность,
        // проба видела в этом направлении ближе, чем до нас.
        float3 fromProbe = biased - probePos;
        float r = length(fromProbe);
        float2 moments = dist.SampleLevel(s, DDGIAtlasUV(p, fromProbe / max(r, 1e-4), v, DDGI_DIST_N, v.distTexel), 0).rg;
        float mean = moments.x;
        float variance = abs(mean * mean - moments.y);
        float cheb = 1.0;
        if (r > mean) {
            float d = r - mean;
            cheb = variance / (variance + d * d);
            cheb = max(cheb * cheb * cheb, 0.0);
        }
        w *= max(0.05, cheb);

        float4 e = irr.SampleLevel(s, DDGIAtlasUV(p, n, v, DDGI_IRR_N, v.irrTexel), 0);
        if (!DDGIFinite3(e.rgb) || !(w == w)) {
            continue;
        }
        // Проба внутри геометрии (альфа 0) не участвует.
        w *= e.a > 0.5 ? 1.0 : 0.0;
        // Совсем малые веса гасим плавно (иначе выброс от далёкой пробы).
        const float crush = 0.2;
        if (w < crush) {
            w *= w * w / (crush * crush);
        }
        sum += e.rgb * w;
        wsum += w;
    }
    if (wsum <= 1e-6) {
        return float4(0.0, 0.0, 0.0, 0.0);
    }
    return float4(sum / wsum, 1.0);
}
