#version 450 core
in vec2 vTexCoord;
out vec4 fragColor;

uniform sampler2D uTex;

void main()
{
    fragColor = texture(uTex, vTexCoord);
}
