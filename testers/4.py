# test_compile.py
import os
from alkash3d.graphics.utils import d3d12_wrapper as dx

def main():
    # 1. Инициализируем устройство (уж естb уже проверено)
    dev = dx.create_device()
    if not dev:
        raise RuntimeError("no device")
    print(f"[py] device: {dev:#x}")

    # 2. Компилируем шейдер
    vs_blob = dx.compile_hlsl(
        os.path.join("../resources", "shaders", "forward_vert.hlsl"),
        "VSMain",
        "vs_5_0"
    )
    print(f"[py] VS blob: {vs_blob:#x}")

    # 3. (необязательно) создаём PSO, чтоб убедиться, что указатель рабочий
    ps_blob = dx.compile_hlsl(
        os.path.join("../resources", "shaders", "forward_frag.hlsl"),
        "PSMain",
        "ps_5_0",
    )
    pso = dx.create_graphics_ps(dev, vs_blob, ps_blob)
    print(f"[py] PSO: {pso:#x}")

    # очистка
    dx.release_resource(pso)
    dx.release_resource(vs_blob)
    dx.release_resource(ps_blob)
    dx.release_resource(dev)

if __name__ == "__main__":
    main()
