// deferred_geom_frag.hlsl
// Выводит 4 render‑targets:
// 0 – позиция (float4, RGBA32F)
// 1 – нормаль  (float4, RGBA16F) – нормаль упакована в [-1,1] → [0,1]
// 2 – альбедо (float4, RGBA8)
// 3 – материал (metallic, roughness, ao, padding)  (float4, RGBA8)

Texture2D albedoMap   : register(t0);
Texture2D normalMap   : register(t1);
Texture2D metallicMap : register(t2);
Texture2D roughMap    : register(t3);
Texture2D aoMap       : register(t4);
SamplerState sLinear  : register(s0);

cbuffer MaterialCB : register(b2)
{
    float4  baseAlbedo;   // fallback‑цвет, если текстура отсутствует
    float   metallic;     // fallback‑значения, если текстура отсутствует
    float   roughness;
    float   ao;
    float3  emissive;
    float   padding;
};

struct PS_IN
{
    float4 posWS : TEXCOORD0;
    float3 normWS : TEXCOORD1;
    float2 uv    : TEXCOORD2;
};

float4 EncodeNormal(float3 n)
{
    // Переводим из [-1,1] в [0,1] и упаковываем в 4‑канальный вектор
    return float4(n * 0.5 + 0.5, 1.0);
}

float4 PSMain(PS_IN input) : SV_Target0
{
    // ---- G‑buffer 0 – позиция
    float4 outPos = float4(input.posWS.xyz, 1.0);

    // ---- G‑buffer 1 – нормаль
    float3 normal = input.normWS;
    // Если есть normal‑map – используем её (если UV заданы)
    if (normalMap.Sample(sLinear, input.uv).a != 0.0) // простая проверка наличия
    {
        float3 nMap = normalMap.Sample(sLinear, input.uv).rgb * 2.0 - 1.0;
        normal = normalize(mul((float3x3)uModel, nMap));
    }
    float4 outNormal = EncodeNormal(normal);

    // ---- G‑buffer 2 – альбедо
    float4 albedo = albedoMap.Sample(sLinear, input.uv);
    if (albedo.a == 0)        // fallback, если texture не привязан
        albedo = baseAlbedo;

    // ---- G‑buffer 3 – материал
    float metallic  = metallicMap.Sample(sLinear, input.uv).r;
    float roughness = roughMap.Sample(sLinear, input.uv).r;
    float aoVal    = aoMap.Sample(sLinear, input.uv).r;

    // Если карты отсутствуют – используем константы из cbuffer
    if (metallicMap.Sample(sLinear, input.uv).a == 0) metallic = metallic;
    if (roughMap.Sample(sLinear, input.uv).a   == 0) roughness = roughness;
    if (aoMap.Sample(sLinear, input.uv).a      == 0) aoVal = ao;

    float4 outMaterial = float4(metallic, roughness, aoVal, 0.0);

    // Пишем сразу в 4 RTV (по порядку, объявленному в pso_mod)
    // Возвратом функции может быть любой из них – движок использует их через
    // привязанные RTV в swap‑chain. Здесь просто возвращаем первый.
    return outPos; // фактически компилятор знает, что есть 4 целевых регистров
}

// Указываем, что шейдер пишет в 4 render‑targets (DX12)
// Порядок соответствуют:
//    SV_Target0 – позиция    (R8G8B8A8_UNORM)
//    SV_Target1 – нормаль     (R8G8B8A8_UNORM)
//    SV_Target2 – альбедо     (R8G8B8A8_UNORM)
//    SV_Target3 – материал    (R8G8B8A8_UNORM)
