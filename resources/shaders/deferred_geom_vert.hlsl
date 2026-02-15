// deferred_geom_vert.hlsl
// Заполняет G‑buffer (позиция, нормаль, альбедо, параметры материала)

cbuffer CameraCB : register(b0)
{
    float4x4 uView;
    float4x4 uProj;
};

cbuffer ModelCB : register(b1)
{
    float4x4 uModel;
};

struct VS_IN
{
    float3 pos     : POSITION;   // позиция вершины
    float3 norm    : NORMAL;    // нормаль (если её нет – будет 0
    float2 tex     : TEXCOORD0; // UV (если её нет – будет 0)
};

struct VS_OUT
{
    float4 posH    : SV_Position; // позиция в клип‑пространстве
    float4 posWS   : TEXCOORD0;   // позиция в мировом пространстве
    float3 normWS  : TEXCOORD1;   // нормаль в мировом пространстве
    float2 uv      : TEXCOORD2;   // UV‑координаты
};

VS_OUT VSMain(VS_IN input)
{
    VS_OUT o;

    // world‑space позиция
    float4 worldPos = mul(uModel, float4(input.pos,1.0));
    o.posWS = worldPos;

    // view‑space → clip‑space
    float4 viewPos = mul(uView, worldPos);
    o.posH = mul(uProj, viewPos);

    // трансформируем нормаль (только вращение/масштаб)
    o.normWS = normalize(mul((float3x3)uModel, input.norm));

    o.uv = input.tex;
    return o;
}
