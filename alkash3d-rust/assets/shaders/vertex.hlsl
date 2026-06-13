//=================================================================================================
// Вершинный шейдер для цветного треугольника
//=================================================================================================

struct VSInput {
    float4 position : POSITION;  // Позиция вершины (x, y, z, w)
    float4 color : COLOR;         // Цвет вершины (r, g, b, a)
};

struct VSOutput {
    float4 position : SV_POSITION; // Позиция в NDC пространстве
    float4 color : COLOR;           // Цвет для пиксельного шейдера
};

VSOutput main(VSInput input) {
    VSOutput output;

    // Передаём позицию как есть (уже в NDC)
    output.position = input.position;

    // Передаём цвет
    output.color = input.color;

    return output;
}