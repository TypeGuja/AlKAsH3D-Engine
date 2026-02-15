// quad_frag.hlsl
// Копирует текстуру (т‑0) в back‑buffer

Texture2D srcTex    : register(t0);
SamplerState sLinear : register(s0);

float4 PSMain(float2 uv : TEXCOORD0) : SV_Target
{
    // Простая выборка и гамма‑коррекция
    float4 col = srcTex.Sample(sLinear, uv);
    // Тонемаппинг (здесь — простейший «reinhard»)
    col.rgb = col.rgb / (col.rgb + 1.0);
    // Гамма‑коррекция (γ≈2.2)
    col.rgb = pow(col.rgb, 1.0/2.2);
    return col;
}
