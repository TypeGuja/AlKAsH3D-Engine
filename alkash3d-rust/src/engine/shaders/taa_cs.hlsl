// Временное сглаживание (TAA) и досборка разрешения (TAAU). Каждый кадр
// камера сдвинута на долю пикселя (последовательность Халтона 2,3), поэтому
// пиксель от кадра к кадру видит разные точки внутри себя. Накопленная
// история даёт то же, что много сэмплов на пиксель: тонкие балясины,
// провода и блики не мерцают, шум мягких теней и сенсора усредняется.
//
// ДОБАВЛЕНО (TAAU): кадр рисуется в пониженном разрешении (renderSize), а
// история и выход — в разрешении экрана (outSize). Пиксель выхода берёт
// сэмплы кадра 3x3 вокруг себя с весом по расстоянию от своего центра до
// их настоящих (сдвинутых) точек: за несколько кадров сдвиги покрывают
// весь пиксель экрана, и собирается полное разрешение. Сэмпл далеко от
// центра — вклад кадра меньше, больше доверия истории. При renderSize ==
// outSize это обычный TAA.
//
// История берётся там, где эта точка мира была в прошлом кадре
// (репроекция через глубину — сцена в основном неподвижна, движется
// камера), и ограничивается разбросом цветов соседей текущего кадра
// (variance clipping в YCoCg) — где история не похожа на настоящее
// (открылся новый край, мигнула лампа), она отбрасывается, а не
// оставляет «призрак».

Texture2D<float4> ColorTex : register(t0);
#ifdef MSAA
Texture2DMS<float> DepthTex : register(t1);
float LoadDepth(int2 p) { return DepthTex.Load(p, 0); }
#else
Texture2D<float> DepthTex : register(t1);
float LoadDepth(int2 p) { return DepthTex.Load(int3(p, 0)); }
#endif
// ДОБАВЛЕНО (TAAU): SSAO применяется здесь, к кадру в его разрешении, а не
// в tonemap — там кадр уже в разрешении экрана, а рассеянный свет нет, и
// вычитание по несовпадающим пикселям давало бы ореолы на краях.
Texture2D<float4> AmbientTex : register(t2);
Texture2D<float4> SSAOTex : register(t3);
Texture2D<float4> HistoryTex : register(t4);
RWTexture2D<float4> Output : register(u0);
SamplerState LinearClamp : register(s0);

cbuffer TaaConstants : register(b0) {
    float4x4 invViewProj;   // текущая, без сдвига — мир из глубины
    float4x4 prevViewProj;  // прошлого кадра, без сдвига (история — без него)
    float2 renderSize;
    float2 outSize;
    float2 jitter;          // сдвиг этого кадра, пиксели кадра (-0.5..0.5)
    float historyValid;
    float feedback;         // доля истории (0.9 — ~10 кадров)
    float applyAO;          // 1 — вычесть затенённый SSAO рассеянный свет
    float3 _pad;
};

float Luma(float3 c) { return dot(c, float3(0.2126, 0.7152, 0.0722)); }

// Яркое HDR сжимаем перед смешиванием (вес 1/(1+L)), иначе одна вспышка
// лампы перетягивает среднее — и разжимаем обратно.
float3 Compress(float3 c) { return c / (1.0 + Luma(c)); }
float3 Expand(float3 c) { return c / max(1.0 - Luma(c), 1e-4); }

float3 ToYCoCg(float3 c) {
    return float3(dot(c, float3(0.25, 0.5, 0.25)), dot(c, float3(0.5, 0.0, -0.5)), dot(c, float3(-0.25, 0.5, -0.25)));
}
float3 FromYCoCg(float3 c) {
    return float3(c.x + c.y - c.z, c.x + c.z, c.x - c.y - c.z);
}

// Бикубический фильтр Катмулла–Рома пятью билинейными выборками (Jimenez):
// билинейная история за десятки кадров замыливает картинку.
float3 SampleHistory(float2 uv) {
    float2 size = outSize;
    float2 pos = uv * size;
    float2 c = floor(pos - 0.5) + 0.5;
    float2 f = pos - c;
    float2 w0 = f * (-0.5 + f * (1.0 - 0.5 * f));
    float2 w1 = 1.0 + f * f * (-2.5 + 1.5 * f);
    float2 w2 = f * (0.5 + f * (2.0 - 1.5 * f));
    float2 w3 = f * f * (-0.5 + 0.5 * f);
    float2 w12 = w1 + w2;
    float2 t0 = (c - 1.0) / size;
    float2 t3 = (c + 2.0) / size;
    float2 t12 = (c + w2 / w12) / size;
    float3 r = 0.0;
    r += HistoryTex.SampleLevel(LinearClamp, float2(t12.x, t0.y), 0).rgb * (w12.x * w0.y);
    r += HistoryTex.SampleLevel(LinearClamp, float2(t0.x, t12.y), 0).rgb * (w0.x * w12.y);
    r += HistoryTex.SampleLevel(LinearClamp, float2(t12.x, t12.y), 0).rgb * (w12.x * w12.y);
    r += HistoryTex.SampleLevel(LinearClamp, float2(t3.x, t12.y), 0).rgb * (w3.x * w12.y);
    r += HistoryTex.SampleLevel(LinearClamp, float2(t12.x, t3.y), 0).rgb * (w12.x * w3.y);
    float wsum = w12.x * w0.y + w0.x * w12.y + w12.x * w12.y + w3.x * w12.y + w12.x * w3.y;
    return max(r / max(wsum, 1e-4), 0.0);
}

bool Finite(float3 v) { return all(v == v) && all(abs(v) < 1e30); }

// Цвет пикселя кадра q: с SSAO (затенённая часть рассеянного света вычтена).
// SSAO — в своём разрешении (половина кадра), четыре билинейные выборки по
// диагоналям: узор поворота ядра SSAO привязан к экрану, без размытия он
// лежал бы на кадре неподвижным шумом.
float3 LoadColor(int2 q) {
    float3 c = ColorTex.Load(int3(q, 0)).rgb;
    if (applyAO > 0.5) {
        float2 uv = (float2(q) + 0.5) / renderSize;
        uint aoW, aoH;
        SSAOTex.GetDimensions(aoW, aoH);
        float2 t = 1.0 / float2(aoW, aoH);
        float3 ao = 0.25 * (SSAOTex.SampleLevel(LinearClamp, uv + t * float2(-1.0, -1.0), 0).rgb
            + SSAOTex.SampleLevel(LinearClamp, uv + t * float2(1.0, -1.0), 0).rgb
            + SSAOTex.SampleLevel(LinearClamp, uv + t * float2(-1.0, 1.0), 0).rgb
            + SSAOTex.SampleLevel(LinearClamp, uv + t * float2(1.0, 1.0), 0).rgb);
        c = max(c - AmbientTex.Load(int3(q, 0)).rgb * (1.0 - ao), 0.0);
    }
    return Finite(c) ? c : 0.0;
}

[numthreads(8, 8, 1)]
void main(uint3 id : SV_DispatchThreadID) {
    int2 p = int2(id.xy);
    if (p.x >= (int)outSize.x || p.y >= (int)outSize.y) {
        return;
    }
    int2 maxQ = int2(renderSize) - 1;
    float2 uvOut = (float2(p) + 0.5) / outSize;
    // центр пикселя выхода в пикселях кадра; пиксель кадра q видит точку
    // экрана q + 0.5 - jitter (камера сдвинута на +jitter)
    float2 c = uvOut * renderSize;
    int2 q0 = int2(floor(c + jitter));
    // пиксели экрана на пиксель кадра: фильтр шире при досборке
    float scale = outSize.x / renderSize.x;

    // соседи 3x3: взвешенный текущий цвет, среднее и разброс для отсечения
    // истории; ближайшая к камере глубина — по ней репроекция (край тонкой
    // балясины берёт её движение, а не фона)
    float3 sum = 0.0;
    float wsum = 0.0, wNear = 0.0;
    float3 m1 = 0.0, m2 = 0.0;
    float closest = 1.0;
    float2 closestUV = uvOut;
    [unroll] for (int y = -1; y <= 1; y++) {
        [unroll] for (int x = -1; x <= 1; x++) {
            int2 q = clamp(q0 + int2(x, y), 0, maxQ);
            float3 col = LoadColor(q);
            float2 at = float2(q) + 0.5 - jitter;
            float2 d = (at - c) * scale;
            // приближение Блэкмана–Харриса гауссом (как у Karis, UE4 TAA)
            float w = exp(-2.29 * dot(d, d));
            sum += col * w;
            wsum += w;
            wNear = max(wNear, w);
            float3 yc = ToYCoCg(Compress(col));
            m1 += yc;
            m2 += yc * yc;
            float dep = LoadDepth(q);
            if (dep < closest) {
                closest = dep;
                closestUV = at / renderSize;
            }
        }
    }
    // при родном разрешении — свой пиксель как есть (как прежний TAA):
    // взвешенное 3x3 мягче и замылило бы кадр
    float3 cur = scale < 1.001 ? LoadColor(clamp(p, 0, maxQ)) : sum / max(wsum, 1e-4);
    float3 mean = m1 / 9.0;
    float3 sigma = sqrt(max(m2 / 9.0 - mean * mean, 0.0));

    float4 ndc = float4(closestUV.x * 2.0 - 1.0, 1.0 - closestUV.y * 2.0, closest, 1.0);
    float4 world = mul(invViewProj, ndc);
    world /= world.w;
    float4 prevClip = mul(prevViewProj, float4(world.xyz, 1.0));
    float2 prevNdc = prevClip.xy / prevClip.w;
    float2 prevUV = float2(prevNdc.x * 0.5 + 0.5, 0.5 - prevNdc.y * 0.5);
    // сдвиг между точкой глубины и центром пикселя выхода — в сторону истории
    prevUV += uvOut - closestUV;

    float3 outC = Compress(cur);
    if (historyValid > 0.5 && prevClip.w > 0.0 && all(prevUV > 0.0) && all(prevUV < 1.0)) {
        float3 hist = SampleHistory(prevUV);
        if (Finite(hist)) {
            float3 h = ToYCoCg(Compress(hist));
            // отсечение истории по разбросу соседей: луч из среднего к истории
            // до границы коробки mean ± 1.25 sigma
            float3 extent = sigma * 1.25 + 1e-4;
            float3 dir = h - mean;
            float3 t = abs(dir) / extent;
            float k = max(t.x, max(t.y, t.z));
            if (k > 1.0) {
                h = mean + dir / k;
            }
            // быстрое движение по экрану — меньше доверия истории (иначе шлейф)
            float speed = length((prevUV - uvOut) * outSize);
            float fb = lerp(feedback, 0.6, saturate(speed / 30.0));
            // ни один сэмпл кадра не попал близко к центру пикселя экрана —
            // вклад кадра меньше (при обычном TAA ближайший всегда не
            // дальше √0.5 пикселя, wNear ≥ exp(-2.29·0.5) = 0.318 — как было)
            float curW = (1.0 - fb) * saturate(wNear / 0.318);
            outC = FromYCoCg(lerp(h, ToYCoCg(Compress(cur)), curW));
        }
    }
    Output[p] = float4(max(Expand(outC), 0.0), 1.0);
}
