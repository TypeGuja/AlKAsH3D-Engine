# test_full.py
import os
from alkash3d.graphics.utils import d3d12_wrapper as dx

def main():
    # 1️⃣ Устройство
    dev = dx.create_device()
    print(f"[py] device = {dev:#x}")

    # 2️⃣ Компиляция шейдеров
    vs = dx.compile_hlsl(
        os.path.join('../resources', 'shaders', 'forward_vert.hlsl'),
        'VSMain',
        'vs_5_0')
    ps = dx.compile_hlsl(
        os.path.join('../resources', 'shaders', 'forward_frag.hlsl'),
        'PSMain',
        'ps_5_0')
    print(f"[py] VS = {vs:#x}, PS = {ps:#x}")

    # 3️⃣ PSO
    pso = dx.create_graphics_ps(dev, vs, ps)
    if not pso:
        raise RuntimeError("CreateGraphicsPipelineState FAILED")
    print(f"[py] PSO = {pso:#x}")

    # 4️⃣ Очистка
    dx.release_resource(pso)
    dx.release_resource(vs)
    dx.release_resource(ps)
    dx.release_resource(dev)
    print("[py] all resources released – success")

if __name__ == '__main__':
    main()
