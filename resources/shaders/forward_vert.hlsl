// ---------------------------------------------------
// Forward‑pipeline – Vertex Shader
// ---------------------------------------------------

// Констант‑буфер (b0). Смещения согласованы с
// Shader._MAT_OFFSETS (uView=0, uProj=64, uModel=128, uTint=192, uTime=208, uNumLights=212)
cbuffer FrameData : register(b0)
{
    float4x4 uView;      // 0‑й offset
    float4x4 uProj;      // 64‑й offset
    float4x4 uModel;     // 128‑й offset
    float4   uTint;      // 192‑й offset (не используется в VS)
    float    uTime;      // 208‑й offset (не используется)
    uint     uNumLights; // 212‑й offset (не используется)
};

// Входные атрибуты – Position, Normal, TexCoord.
// Их порядок и семантики должны совпадать с layout‑ом, объявленным в pso_mod.rs.
struct VS_IN
{
    float3 pos  : POSITION;   // xyz
    float3 norm : NORMAL;    // xyz (можно игнорировать)
    float2 tex  : TEXCOORD0;  // uv
};

struct VS_OUT
{
    float4 pos : SV_POSITION; // уже в clip‑пространстве
    float2 tex : TEXCOORD0;   // передаём UV дальше
};

// ---------------------------------------------------
// Точка входа – имя ЖЁСТКО фиксировано в бекенде.
// ---------------------------------------------------
VS_OUT VSMain(VS_IN i)
{
    VS_OUT o;

    // 1) Переводим вершину в мир
    float4 worldPos = mul(uModel, float4(i.pos, 1.0));

    // 2) Видеокамера
    float4 viewPos  = mul(uView, worldPos);

    // 3) Проекция
    o.pos = mul(uProj, viewPos);

    // UV просто копируем (если они нулевые – будет (0,0))
    o.tex = i.tex;

    // нормаль пока не используется, можем её просто игнорировать.
    return o;
}
