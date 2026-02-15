# test_pso.py - ИСПРАВЛЕННАЯ ВЕРСИЯ
import os
import ctypes
from pathlib import Path
from alkash3d.graphics.utils import d3d12_wrapper as dx


def main():
    # 1️⃣  Device
    device = dx.create_device()
    if not device:
        raise RuntimeError("Failed to create D3D12 device")
    print(f"Device: 0x{device:016x}")

    # 2️⃣  Compile shaders - ТОЛЬКО ЧЕРЕЗ Python обертку!
    print("Compiling vertex shader...")
    vs = dx.compile_hlsl(
        "../resources/shaders/forward_vert.hlsl",
        "VSMain",
        "vs_5_0"
    )
    print(f"VS blob: 0x{vs:016x}")

    print("Compiling pixel shader...")
    ps = dx.compile_hlsl(
        "../resources/shaders/forward_frag.hlsl",
        "PSMain",
        "ps_5_0"
    )
    print(f"PS blob: 0x{ps:016x}")

    # 3️⃣  Build PSO
    print("Creating PSO...")
    pso = dx.create_graphics_ps(device, vs, ps)
    if not pso:
        raise RuntimeError("CreateGraphicsPipelineState FAILED")
    print(f"PSO created: 0x{pso:016x}")

    # 4️⃣  Clean‑up
    dx.release_resource(pso)
    dx.release_resource(vs)
    dx.release_resource(ps)
    dx.release_resource(device)
    print("Cleanup complete")


if __name__ == "__main__":
    main()