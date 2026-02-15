// входные данные от VS
struct VS_OUT
{
    float4 pos : SV_POSITION;
    float2 uv  : TEXCOORD0;
};

Texture2D   gAlbedo  : register(t0);   // SRV – слот 1 в root‑signature
SamplerState gSampler : register(s0); // статический сэмплер (в root‑signature)

float4 PSMain(VS_OUT i) : SV_TARGET
{
    // читаем цвет из текстуры
    return gAlbedo.Sample(gSampler, i.uv);
}
