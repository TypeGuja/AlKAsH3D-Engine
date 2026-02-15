"""
Простейший менеджер HLSL‑шейдеров для DirectX 12.
* Компилирует VS/PS через DX12‑бекенд.
* Создаёт один constant‑buffer, в который записываются матрицы.
"""

import os
import numpy as np
from alkash3d.utils import logger
from alkash3d.graphics.dx12_backend import DX12Backend

class Shader:
    """Обёртка над парой VS/PS‑blob‑ов и готовым PSO."""
    _MAT_OFFSETS = {
        "uView": 0,
        "uProj": 64,
        "uModel": 128,
    }
    _CB_SIZE = 192

    def __init__(self, backend: DX12Backend, vertex_path: str, fragment_path: str):
        self.backend = backend

        print("=" * 50)
        print("[Shader] Initializing shader program")
        print(f"[Shader] Vertex shader path: {vertex_path}")
        print(f"[Shader] Fragment shader path: {fragment_path}")
        print("=" * 50)

        print("[Shader] Compiling vertex shader...")
        self.vs_blob = backend.compile_shader("vs", vertex_path)
        if not self.vs_blob:
            raise RuntimeError(f"Failed to compile vertex shader: {vertex_path}")
        print(f"[Shader] Vertex shader compiled: {hex(self.vs_blob)}")

        print("[Shader] Compiling fragment shader...")
        self.ps_blob = backend.compile_shader("ps", fragment_path)
        if not self.ps_blob:
            raise RuntimeError(f"Failed to compile fragment shader: {fragment_path}")
        print(f"[Shader] Fragment shader compiled: {hex(self.ps_blob)}")

        print("[Shader] Creating graphics pipeline...")
        self.pso = backend.create_graphics_ps(self.vs_blob, self.ps_blob)
        if not self.pso:
            raise RuntimeError("Failed to create graphics pipeline")
        print(f"[Shader] Graphics pipeline created: {hex(self.pso)}")
        print("=" * 50)

        self._frame_cb = backend.create_constant_buffer(
            b"\x00" * self._CB_SIZE
        )

        idx = backend.cbv_srv_uav_heap.next_free()
        cpu_handle = backend.cbv_srv_uav_heap.get_cpu_handle(idx)
        backend.create_shader_resource_view(self._frame_cb, cpu_handle)

        self._frame_cb_gpu = backend.cbv_srv_uav_heap.get_gpu_handle(idx)
        self._frame_data = bytearray(self._CB_SIZE)

    def use(self) -> None:
        self.backend.set_graphics_pipeline(self.pso)

    def set_uniform_mat4(self, name: str, mat) -> None:
        if name not in self._MAT_OFFSETS:
            logger.debug(f"[Shader] Unknown mat4 uniform: {name}")
            return

        arr = np.asarray(mat, dtype=np.float32).reshape(16)
        offset = self._MAT_OFFSETS[name]
        self._frame_data[offset: offset + 64] = arr.tobytes()
        self.backend.update_buffer(self._frame_cb, bytes(self._frame_data))
        self.backend.set_root_descriptor_table(0, self._frame_cb_gpu)

    def set_uniform_vec3(self, name: str, vec) -> None:
        pass

    def set_uniform_int(self, name: str, value: int) -> None:
        pass

    def set_uniform_float(self, name: str, value: float) -> None:
        pass

    def reload_if_needed(self):
        pass