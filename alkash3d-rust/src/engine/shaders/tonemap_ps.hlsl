Texture2D HDRSource : register(t0);
// ДОБАВЛЕНО (bloom): результат extract+blur-прохода (half-res,
// уже билинейно "размазанное" свечение ярких источников) —
// складывается с основным HDR-цветом ДО тонмаппинга, что даёт
// физически правдоподобный эффект "пересвета" вокруг фонарей
// вместо плоских ярких пятен без ореола.
Texture2D BloomSource : register(t1);
// ДОБАВЛЕНО (Фаза 8 плана по реализму/фонарям — volumetric-
// подсветка): результат screen-space raymarch-прохода (half-res,
// см. compile_volumetric_shaders) — аддитивно складывается с
// остальным HDR-цветом ДО тонмаппинга, как и bloom, чтобы god rays
// тоже проходили через один и тот же ACES-тонмаппинг, а не
// накладывались поверх уже сжатого LDR-изображения (что выглядело
// бы плоско и не сочеталось по яркости с остальной сценой).
Texture2D VolumetricSource : register(t2);
// ДОБАВЛЕНО (максимальная графика — SSAO, см. engine/pipeline_ssao.rs):
// half-res множитель [0,1] контактной окклюзии — применяется к
// ОСНОВНОМУ (не bloom/volumetric — те источники света/атмосфера, не
// заслоняемая геометрией поверхность) цвету ДО суммы с ними, см.
// main() ниже.
Texture2D SSAOSource : register(t3);
// ДОБАВЛЕНО (честный SSAO): только ambient-вклад каждого пикселя
// (SV_Target1 основного прохода, уже внутри HDRSource).
Texture2D AmbientSource : register(t4);
// ДОБАВЛЕНО (локальная экспозиция, engine/local_exposure.rs): сетка 64x36
// средней логарифмической яркости кадра до экспозиции.
Texture2D<float> LocalLumSource : register(t5);
SamplerState PointSampler : register(s0);

cbuffer TonemapConstants : register(b0) {
    float exposure;
    float bloomIntensity;
    float2 _padding;
    // ДОБАВЛЕНО (хроматическая адаптация, AlkashEngine::white_point):
    // строки матрицы 3x3 (единичная — без адаптации).
    float4 adaptRow0;
    float4 adaptRow1;
    float4 adaptRow2;
    // ДОБАВЛЕНО (сумеречное зрение): x — кд/м² на единицу яркости кадра
    // (0 — выключено). y — дисторсия объектива k, z — хроматическая
    // аберрация (AlkashEngine::lens; 0 — выключено).
    float4 visionParams;
    // ДОБАВЛЕНО (локальная экспозиция): x — степень сжатия (0 — выключено),
    // y — средняя яркость кадра после экспозиции, к ней тянем каждое место.
    // z — ДОБАВЛЕНО (TAAU): 1 — SSAO уже вычтен в TAA, здесь не трогать.
    float4 localParams;
    // ДОБАВЛЕНО (размытие в движении): мир из глубины этого кадра, экран
    // прошлого (обе без сдвига TAA); x — доля пути за кадр (0 — выкл),
    // y — предел длины размытия, доля кадра.
    float4x4 mbInvViewProj;
    float4x4 mbPrevViewProj;
    float4 mbParams;
};
Texture2D<float> DepthSource : register(t6);

// Путь точки экрана uv за кадр (uv сейчас минус uv в прошлом кадре): из
// глубины — в мир, оттуда — в экран прошлого кадра. Только движение камеры:
// подвижных тел, заметных на экране, в сценах почти нет.
float2 ScreenVelocity(float2 uv) {
    if (mbParams.x <= 0.0) {
        return 0.0;
    }
    float d = DepthSource.SampleLevel(PointSampler, uv, 0);
    float4 world = mul(mbInvViewProj, float4(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, d, 1.0));
    world /= world.w;
    float4 prev = mul(mbPrevViewProj, float4(world.xyz, 1.0));
    if (prev.w <= 0.0) {
        return 0.0;
    }
    float2 prevUV = float2(prev.x / prev.w * 0.5 + 0.5, 0.5 - prev.y / prev.w * 0.5);
    float2 v = (uv - prevUV) * mbParams.x;
    float len = length(v);
    return len > mbParams.y ? v * (mbParams.y / len) : v;
}

// Средняя яркость окрестности (до экспозиции): сетка 64x36, билинейно и
// ещё ядром 3x3 (1-2-1) с шагом в клетку — плавно, без ступенек клеток.
float LocalLuminance(float2 uv) {
    float2 t = 1.0 / float2(64.0, 36.0);
    float s = 0.0;
    [unroll] for (int y = -1; y <= 1; y++) {
        [unroll] for (int x = -1; x <= 1; x++) {
            float w = (x == 0 ? 2.0 : 1.0) * (y == 0 ? 2.0 : 1.0);
            s += LocalLumSource.SampleLevel(PointSampler, uv + t * float2(x, y), 0) * w;
        }
    }
    return exp(s / 16.0);
}

struct PS_INPUT {
    float4 pos : SV_POSITION;
    float2 uv : TEXCOORD0;
};

// ACES filmic tonemap, аппроксимация Krzysztof Narkowicz (2015) —
// стандартная в игровой индустрии формула, недорогая (никаких
// циклов/textur-выборок сверх одной), даёт кинематографичную
// компрессию яркости с мягким "плечом" у самых ярких значений
// вместо жёсткого обрезания.
float3 ACESFilm(float3 x) {
    float a = 2.51;
    float b = 0.03;
    float c = 2.43;
    float d = 0.59;
    float e = 0.14;
    return saturate((x * (a * x + b)) / (x * (c * x + d) + e));
}

// ДОБАВЛЕНО (точный ACES, adaptRow2.w > 0.5): фит Стивена Хилла для
// RRT+ODT (как в UE4) — с переходом в цветовое пространство ACES и
// обратно; не пересыщает яркое и не уводит оттенки, как короткая
// формула выше. Вход масштабирован так, чтобы средне-серый остался там же.
static const float3x3 ACES_IN = {
    { 0.59719, 0.35458, 0.04823 },
    { 0.07600, 0.90834, 0.01566 },
    { 0.02840, 0.13383, 0.83777 }
};
static const float3x3 ACES_OUT = {
    { 1.60475, -0.53108, -0.07367 },
    { -0.10208, 1.10813, -0.00605 },
    { -0.00327, -0.07276, 1.07602 }
};
float3 ACESHill(float3 c) {
    c = mul(ACES_IN, c * 2.4);
    float3 a = c * (c + 0.0245786) - 0.000090537;
    float3 b = c * (0.983729 * c + 0.4329510) + 0.238081;
    return saturate(mul(ACES_OUT, a / b));
}

// ДОБАВЛЕНО (объектив): бочкообразная дисторсия r' = r(1 + k r²) в
// координатах с учётом аспекта; нормировка держит углы кадра на месте
// (иначе по краям выборка уходит за кадр). `scale` — разный для R/G/B:
// поперечная хроматическая аберрация, каналы расходятся к краю.
float2 LensUV(float2 uv, float k, float scale) {
    float aspect = _padding.y > 0.0 ? _padding.y : 1.0;
    float2 d = (uv - 0.5) * float2(aspect, 1.0);
    float rc2 = 0.25 * (aspect * aspect + 1.0);
    float f = (1.0 + k * dot(d, d)) / (1.0 + k * rc2) * scale;
    return 0.5 + d * f / float2(aspect, 1.0);
}

// HDR-цвет сцены в точке кадра uv: основной цвет с SSAO, bloom, объёмный
// свет и хроматическая адаптация — всё, что дальше идёт в экспозицию.
float3 SceneHDR(float2 uv, float2 vel) {
    float3 hdrColor = HDRSource.Sample(PointSampler, uv).rgb;
    // ДОБАВЛЕНО (размытие в движении): выдержка видит точку на протяжении
    // части пути — 8 выборок вдоль него, со сдвигом от пикселя к пикселю
    // (иначе ступеньки), меньше полупикселя — не размываем
    uint hw, hh;
    HDRSource.GetDimensions(hw, hh);
    if (dot(vel * float2(hw, hh), vel * float2(hw, hh)) > 0.25) {
        float n = frac(52.9829189 * frac(dot(uv * float2(hw, hh), float2(0.06711056, 0.00583715))));
        float3 acc = 0.0;
        [unroll] for (int i = 0; i < 8; i++) {
            float t = (i + n) / 8.0 - 0.5;
            acc += HDRSource.SampleLevel(PointSampler, uv - vel * t, 0).rgb;
        }
        hdrColor = acc / 8.0;
    }
    // ДОБАВЛЕНО (SSAO): честное разделение "только ambient-член"
    // потребовало бы depth pre-pass'а (отдельная доработка) — здесь
    // AO-множитель затемняет ВЕСЬ поверхностный цвет (диффуз +
    // specular + ambient), не только ambient. Задокументированное
    // упрощение, тот же уровень, что и у остальных пост-эффектов
    // этого движка.
    //
    // ИСПРАВЛЕНО (честный SSAO): AO — это доля НЕБА/окружения,
    // видимая из точки, поэтому она гасит только рассеянный
    // (ambient) свет. Прямой свет солнца и фонарей уже честно
    // затенён shadow map'ами — умножать его на AO значит затенять
    // дважды (тёмные "грязные" углы на ярком солнце). Вычитаем из
    // HDR ровно затенённую часть ambient: hdr - ambient * (1 - AO).
    // ИЗМЕНЕНО: четыре билинейные выборки по диагоналям (≈ коробка 4x4
    // текселя SSAO) — узор поворота ядра у SSAO привязан к экрану и
    // проходит мимо TAA, без размытия он лежит на кадре неподвижным шумом.
    if (localParams.z < 0.5) {
        uint aoW, aoH;
        SSAOSource.GetDimensions(aoW, aoH);
        float2 aoT = 1.0 / float2(aoW, aoH);
        float3 aoColor = 0.25 * (SSAOSource.Sample(PointSampler, uv + aoT * float2(-1.0, -1.0)).rgb
            + SSAOSource.Sample(PointSampler, uv + aoT * float2(1.0, -1.0)).rgb
            + SSAOSource.Sample(PointSampler, uv + aoT * float2(-1.0, 1.0)).rgb
            + SSAOSource.Sample(PointSampler, uv + aoT * float2(1.0, 1.0)).rgb);
        float3 ambientColor = AmbientSource.Sample(PointSampler, uv).rgb;
        hdrColor = max(hdrColor - ambientColor * (1.0 - aoColor), 0.0);
    }
    // BloomSource — half-res текстура. ИЗМЕНЕНО (bloom цепочкой
    // уровней): статический сэмплер s0 теперь билинейный (имя
    // PointSampler осталось историческим) — растяжение на экран
    // гладкое, без квадратов 2x2; полноразмерные источники читаются
    // в центрах пикселей, для них это то же, что point.
    float3 bloomColor = BloomSource.Sample(PointSampler, uv).rgb;
    // ДОБАВЛЕНО (Фаза 8): volumetric-свет — тоже half-res, тот же
    // point-сэмплер + upscale "как есть" (raymarch сам по себе уже
    // достаточно гладкий по построению, см. jitter в
    // compile_volumetric_shaders — дополнительный билинейный
    // сэмплер здесь не добавляет заметного качества).
    float3 volumetricColor = VolumetricSource.Sample(PointSampler, uv).rgb;
    float3 combined = hdrColor + bloomColor * bloomIntensity + volumetricColor;
    return max(float3(dot(adaptRow0.xyz, combined), dot(adaptRow1.xyz, combined), dot(adaptRow2.xyz, combined)), 0.0);
}

// Зрение, экспозиция, виньетирование, шум сенсора, тонмаппинг и sRGB.
float4 Finish(PS_INPUT input, float3 combined) {
    // ДОБАВЛЕНО (сумеречное зрение, эффект Пуркинье): ниже ~3 кд/м²
    // колбочки сдают, работают палочки — цвет уходит, остаётся яркость
    // с лёгким сдвигом в синее (палочки чувствительнее к сине-зелёному).
    // Считается по абсолютной яркости сцены, до экспозиции: глаз может
    // привыкнуть к темноте, но цвета в ней не вернёт.
    if (visionParams.x > 0.0) {
        float L = dot(combined, float3(0.2126, 0.7152, 0.0722)) * visionParams.x;
        float photopic = smoothstep(-2.0, 0.5, log10(max(L, 1e-6)));
        float scot = dot(combined, float3(0.04, 0.55, 0.41));
        float3 rod = scot * float3(0.85, 0.95, 1.12);
        combined = lerp(rod, combined, photopic);
    }
    float3 exposed = combined * exposure;
    // ДОБАВЛЕНО (локальная экспозиция): место темнее среднего по кадру —
    // светлее, ярче — темнее, (окрестность/среднее)^-k; множитель плавный
    // (окрестность ~ клетки сетки), мелкий контраст остаётся как есть.
    if (localParams.x > 0.0) {
        float local = LocalLuminance(input.uv) * exposure;
        float f = pow(max(local, 1e-5) / max(localParams.y, 1e-5), -localParams.x);
        exposed *= clamp(f, 0.25, 4.0);
    }
    // ДОБАВЛЕНО (камера, AlkashEngine::lens): естественное виньетирование
    // объектива — освещённость кадра падает к краям как cos^4 угла от оси
    // (_padding.x = tan(fov/2), _padding.y = aspect; 0 — выключено).
    if (_padding.x > 0.0) {
        float2 ndc = input.uv * 2.0 - 1.0;
        float2 t = ndc * float2(_padding.x * _padding.y, _padding.x);
        float c2 = 1.0 / (1.0 + dot(t, t));
        exposed *= c2 * c2;
    }
    // Дробовой шум: фотонов на пиксель тем меньше, чем сильнее усиление
    // (экспозиция), разброс ~ sqrt(сигнала * усиления). adaptRow0.w —
    // масштаб (0 — без шума), adaptRow1.w — номер кадра.
    if (adaptRow0.w > 0.0) {
        uint2 px = uint2(input.pos.xy);
        uint h = px.x * 1973u + px.y * 9277u + (uint)adaptRow1.w * 26699u;
        float3 g;
        [unroll] for (int k = 0; k < 3; k++) {
            // сумма двух равномерных — близко к гауссу, без тригонометрии
            h = (h ^ 61u) ^ (h >> 16); h *= 9u; h ^= h >> 4; h *= 0x27d4eb2du; h ^= h >> 15;
            float a = (h & 0xFFFFu) / 65535.0, b = (h >> 16) / 65535.0;
            g[k] = (a + b - 1.0) * 2.45;
        }
        exposed = max(exposed + sqrt(max(exposed, 0.0) * exposure * adaptRow0.w) * g, 0.0);
    }
    float3 tonemapped = adaptRow2.w > 0.5 ? ACESHill(exposed) : ACESFilm(exposed);
    // Гамма-коррекция: back buffer — R8G8B8A8_UNORM без sRGB-вьюхи
    // (см. Renderer::back_buffers/create_pipeline_state — формат
    // тот же, что был и до Фазы 5), поэтому применяем гамму 1/2.2
    // здесь явно, а не полагаемся на автоматическую sRGB-конверсию
    // GPU, которой при этом формате RTV попросту нет.
    // ИЗМЕНЕНО: точная кривая sRGB (линейный участок у чёрного), а не
    // степень 1/2.2 — та в тенях заметно темнее, чем покажет монитор sRGB.
    float3 c = max(tonemapped, 0.0);
    float3 gammaCorrected = c <= 0.0031308 ? c * 12.92 : 1.055 * pow(c, 1.0 / 2.4) - 0.055;
    return float4(gammaCorrected, 1.0);
}

float4 main(PS_INPUT input) : SV_TARGET {
    float k = visionParams.y, ca = visionParams.z;
    // путь точки — по своему пикселю (после дисторсии каналы рядом, путь тот же)
    float2 vel = ScreenVelocity(LensUV(input.uv, k, 1.0));
    float3 combined;
    if (k != 0.0 || ca != 0.0) {
        combined = float3(
            SceneHDR(LensUV(input.uv, k, 1.0 - ca), vel).r,
            SceneHDR(LensUV(input.uv, k, 1.0), vel).g,
            SceneHDR(LensUV(input.uv, k, 1.0 + ca), vel).b);
    } else {
        combined = SceneHDR(input.uv, vel);
    }
    return Finish(input, combined);
}
