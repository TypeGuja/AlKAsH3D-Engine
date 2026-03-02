// ---------------------------------------------------
// Forward‑pipeline – Pixel (Fragment) Shader
// ---------------------------------------------------

// Текстура (t0) и статический сэмплер (s0) – объявлены в root‑signature.
Texture2D uAlbedo  : register(t0);
SamplerState uSampler : register(s0);

// Тот же constant‑buffer, что и в вершинном шейдере.
// Нам нужны только uTint (цвет‑мультипликатор) и, при желании, время/свет.
cbuffer FrameData : register(b0)
{
    float4x4 uView;
    float4x4 uProj;
    float4x4 uModel;
    float4   uTint;      // По умолчанию (1,1,1,1)
    float    uTime;
    uint     uNumLights;
};

struct PS_IN
{
    float4 pos : SV_POSITION;
    float2 tex : TEXCOORD0;
};

// ---------------------------------------------------
// Точка входа – PSMain.
// ---------------------------------------------------
float4 PSMain(PS_IN i) : SV_TARGET
{
    // Если в сцене нет привязанной текстуры, ForwardRenderer создаёт
    // 1×1‑белый placeholder‑texture, поэтому у нас всегда будет корректный Sample().
    float4 col = uAlbedo.Sample(uSampler, i.tex);

    // Применяем цвет‑мультипликатор, заданный из Python:
    // sh.set_uniform_vec4("uTint", (r,g,b,a))
    col *= uTint;

    return col;
}
