cbuffer TransformConstants : register(b0) {
    float4x4 modelViewProj;
    float4x4 model;
    float4x4 view;
    float4x4 proj;
    float4 cameraPos;
    float4 lightDir;
    float4 lightColor;
    float4 ambientColor;
    uint lightCount;
    uint3 _lightCountPadding;
    float4 gridWorldMin; // xyz = world_min, w = cell_size
    uint4 gridDimensions; // x,y,z = grid_width/height/depth
};

// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы): поле `uv` в
// VS_INPUT/VS_OUTPUT — зеркалит новое поле `uv: [f32;2]` в
// `engine::Vertex` (TEXCOORD0 элемент input layout, см. pso.rs).
// TEXCOORD2 у VS_OUTPUT.uv (не TEXCOORD0!) — TEXCOORD0/1 у
// VS_OUTPUT уже заняты worldPos/normal, а входной семантический
// индекс input layout (TEXCOORD0 у VS_INPUT.uv) — ОТДЕЛЬНОЕ
// пространство имён от выходных семантик VS_OUTPUT, переиспользовать
// индексы между входом/выходом вершинного шейдера можно без
// конфликта, но здесь сознательно выбран следующий свободный
// (TEXCOORD2), чтобы не запутывать чтение кода.
// ДОБАВЛЕНО (Задача #15, normal mapping): TANGENT — зеркалит новое
// поле `tangent: [f32;4]` в `engine::Vertex` (xyz + w=handedness,
// см. подробный комментарий там). VS_OUTPUT.tangent — TEXCOORD3
// (следующий свободный после worldPos@0/normal@1/uv@2).
struct VS_INPUT {
    float4 pos : POSITION;
    float3 normal : NORMAL;
    float4 color : COLOR;
    float2 uv : TEXCOORD0;
    float4 tangent : TANGENT;
};
struct VS_OUTPUT {
    float4 pos : SV_POSITION;
    float4 color : COLOR;
    float3 worldPos : TEXCOORD0;
    float3 normal : TEXCOORD1;
    float2 uv : TEXCOORD2;
    float4 tangent : TEXCOORD3;
};
VS_OUTPUT main(VS_INPUT input) {
    VS_OUTPUT output;
    output.pos = mul(modelViewProj, input.pos);
    output.color = input.color;
    output.worldPos = mul(model, input.pos).xyz;
    // Верхний 3x3 блок model — вращение/масштаб, без переноса;
    // этого достаточно для направлений при равномерном scale.
    float3x3 normalMatrix = (float3x3)model;
    output.normal = normalize(mul(normalMatrix, input.normal));
    output.uv = input.uv;
    // ДОБАВЛЕНО (Задача #15, normal mapping): tangent преобразуется
    // ТОЙ ЖЕ normalMatrix, что и normal (оба — направления, не
    // точки, w-компонента handedness переносится как есть — знак
    // не зависит от вращения/равномерного масштаба).
    output.tangent = float4(normalize(mul(normalMatrix, input.tangent.xyz)), input.tangent.w);
    return output;
}
