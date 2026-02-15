// temporal_aa_pass.hlsl – заглушка (pass‑through)
Texture2D srcTex : register(t0);
SamplerState samLinear : register(s0);

float4 main(float4 pos : SV_POSITION, float2 uv : TEXCOORD0) : SV_TARGET
{
    return srcTex.Sample(samLinear, uv);
}
