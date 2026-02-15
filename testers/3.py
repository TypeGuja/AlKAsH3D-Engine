import os

shader_files = [
    r"C:\Users\user\Documents\GitHub\AlKAsH3D-Engine\resources\shaders\forward_vert.hlsl",
    r"C:\Users\user\Documents\GitHub\AlKAsH3D-Engine\resources\shaders\forward_frag.hlsl",
    r"C:\Users\user\Documents\GitHub\AlKAsH3D-Engine\resources\shaders\test.hlsl"
]

for file in shader_files:
    if os.path.exists(file):
        print(f"✓ {file} - существует, размер: {os.path.getsize(file)} байт")
    else:
        print(f"✗ {file} - НЕ СУЩЕСТВУЕТ!")