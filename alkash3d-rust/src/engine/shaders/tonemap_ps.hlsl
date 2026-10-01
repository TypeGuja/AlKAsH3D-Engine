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
SamplerState PointSampler : register(s0);

cbuffer TonemapConstants : register(b0) {
    float exposure;
    float bloomIntensity;
    float2 _padding;
};

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

float4 main(PS_INPUT input) : SV_TARGET {
    float3 hdrColor = HDRSource.Sample(PointSampler, input.uv).rgb;
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
    float3 aoColor = SSAOSource.Sample(PointSampler, input.uv).rgb;
    float3 ambientColor = AmbientSource.Sample(PointSampler, input.uv).rgb;
    hdrColor = max(hdrColor - ambientColor * (1.0 - aoColor), 0.0);
    // BloomSource — half-res текстура, PointSampler здесь всё
    // равно даёт визуально мягкий результат, т.к. само свечение
    // уже размыто предыдущим Gaussian-blur проходом (см.
    // compile_bloom_shaders) — отдельный билинейный сэмплер под
    // апскейл bloom не заводим, чтобы не плодить лишний статический
    // сэмплер только ради этого.
    float3 bloomColor = BloomSource.Sample(PointSampler, input.uv).rgb;
    // ДОБАВЛЕНО (Фаза 8): volumetric-свет — тоже half-res, тот же
    // point-сэмплер + upscale "как есть" (raymarch сам по себе уже
    // достаточно гладкий по построению, см. jitter в
    // compile_volumetric_shaders — дополнительный билинейный
    // сэмплер здесь не добавляет заметного качества).
    float3 volumetricColor = VolumetricSource.Sample(PointSampler, input.uv).rgb;
    float3 combined = hdrColor + bloomColor * bloomIntensity + volumetricColor;
    float3 exposed = combined * exposure;
    float3 tonemapped = ACESFilm(exposed);
    // Гамма-коррекция: back buffer — R8G8B8A8_UNORM без sRGB-вьюхи
    // (см. Renderer::back_buffers/create_pipeline_state — формат
    // тот же, что был и до Фазы 5), поэтому применяем гамму 1/2.2
    // здесь явно, а не полагаемся на автоматическую sRGB-конверсию
    // GPU, которой при этом формате RTV попросту нет.
    float3 gammaCorrected = pow(max(tonemapped, 0.0), 1.0 / 2.2);
    return float4(gammaCorrected, 1.0);
}
