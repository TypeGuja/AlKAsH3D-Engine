// tonemap_pass.hlsl – простейший Reinhard‑тонемаппер (pass‑through)
Texture2D srcTex : register(t0);
SamplerState samLinear : register(s0);

float4 main(float4 pos : SV_POSITION, float2 uv : TEXCOORD0) : SV_TARGET
{
    float3 col = srcTex.Sample(samLinear, uv).rgb;
    col = col / (col + float3(1.0,1.0,1.0));   // Reinhard
    return float4(col, 1.0);
}
