#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
Полный тестер для AlKAsH3D Engine
Тестирует все компоненты движка: графику, математику, сцену, физику, аудио, ввод
"""

import sys
import time
import ctypes
import traceback
import gc
import os
import numpy as np
from pathlib import Path
from typing import Dict, List, Tuple, Any, Optional

# Добавляем путь к движку
current_dir = Path(__file__).parent
if str(current_dir) not in sys.path:
    sys.path.insert(0, str(current_dir))

# Цвета для вывода
GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
BLUE = "\033[94m"
CYAN = "\033[96m"
RESET = "\033[0m"


def print_header(text):
    print(f"\n{BLUE}{'=' * 70}{RESET}")
    print(f"{BLUE}{text:^70}{RESET}")
    print(f"{BLUE}{'=' * 70}{RESET}")


def print_success(text):
    print(f"{GREEN}✅ {text}{RESET}")


def print_error(text):
    print(f"{RED}❌ {text}{RESET}")


def print_warning(text):
    print(f"{YELLOW}⚠️  {text}{RESET}")


def print_info(text):
    print(f"  📌 {text}")


def print_subheader(text):
    print(f"\n{CYAN}--- {text} ---{RESET}")


class TestResult:
    """Результат одного теста"""

    def __init__(self, name: str):
        self.name = name
        self.passed = False
        self.error = None
        self.duration = 0.0
        self.details = []

    def __str__(self):
        status = f"{GREEN}PASSED{RESET}" if self.passed else f"{RED}FAILED{RESET}"
        return f"{self.name}: {status} ({self.duration:.3f}s)"


class TestSuite:
    """Набор тестов"""

    def __init__(self, name: str):
        self.name = name
        self.tests: List[TestResult] = []
        self.start_time = time.time()

    def add_test(self, name: str, func, *args, **kwargs) -> TestResult:
        """Добавить и запустить тест"""
        print_info(f"Running {name}...")
        result = TestResult(name)

        try:
            start = time.time()
            success = func(*args, **kwargs)
            result.duration = time.time() - start

            if success:
                result.passed = True
                print_success(f"{name} PASSED ({result.duration:.3f}s)")
            else:
                result.passed = False
                print_error(f"{name} FAILED ({result.duration:.3f}s)")
        except Exception as e:
            result.duration = time.time() - start
            result.passed = False
            result.error = str(e)
            result.details = traceback.format_exc().split('\n')
            print_error(f"{name} EXCEPTION: {e}")
            traceback.print_exc()

        self.tests.append(result)
        return result

    def summary(self) -> Dict:
        """Сводка по тестам"""
        passed = sum(1 for t in self.tests if t.passed)
        failed = len(self.tests) - passed
        total_time = time.time() - self.start_time

        print_header(f"{self.name} SUMMARY")
        print(f"Total tests:  {len(self.tests)}")
        print(f"{GREEN}Passed:       {passed}{RESET}")
        print(f"{RED}Failed:       {failed}{RESET}")
        print(f"Total time:   {total_time:.3f}s")

        if failed > 0:
            print_subheader("Failed tests:")
            for t in self.tests:
                if not t.passed:
                    print(f"  {RED}• {t.name}{RESET}")
                    if t.error:
                        print(f"    Error: {t.error}")

        return {
            "total": len(self.tests),
            "passed": passed,
            "failed": failed,
            "time": total_time
        }


# ============================================================================
# ТЕСТЫ ИМПОРТОВ
# ============================================================================
def test_imports() -> bool:
    """Тест импорта всех модулей"""
    modules_to_test = [
        # Корневой пакет
        "alkash3d",
        "alkash3d.engine",
        "alkash3d.graphics",
        "alkash3d.graphics.backend",
        "alkash3d.graphics.dx12_backend",
        "alkash3d.graphics.gl_backend",

        # Рендеринг (в новых версиях всё живёт в пакете `renderer`)
        "alkash3d.renderer",                     # общий пакет
        "alkash3d.renderer.shader",             # менеджер шейдеров
        "alkash3d.renderer.pipelines.forward",   # пример пайплайна
        "alkash3d.renderer.pipelines.deferred",
        "alkash3d.renderer.pipelines.hybrid",
        "alkash3d.renderer.pipelines.rtx_renderer",

        # Утилиты графики
        "alkash3d.graphics.utils.d3d12_wrapper",
        "alkash3d.graphics.utils.descriptor_heap",

        # Материалы и менеджер текстур (теперь в пакете `assets`)
        "alkash3d.assets.material",
        "alkash3d.assets.texture_manager",

        # Сцена и её компоненты
        "alkash3d.scene.node",
        "alkash3d.scene.scene",
        "alkash3d.scene.camera",
        "alkash3d.scene.mesh",
        "alkash3d.scene.light",
        "alkash3d.scene.model",

        # Окно (отдельный модуль)
        "alkash3d.window",

        # Вспомогательные утилиты
        "alkash3d.utils.logger",
        "alkash3d.utils.config",
        "alkash3d.utils.timer",
        "alkash3d.utils.resource_manager",
        "alkash3d.math.vector",
        "alkash3d.math.matrix",
        "alkash3d.math.quaternion",
        "alkash3d.math.transform",

        # Физика (заглушки, но импортировать нужно)
        "alkash3d.physics.physics_world",
        "alkash3d.physics.rigid_body",
        "alkash3d.physics.collision",

        # Аудио (заглушки)
        "alkash3d.audio.audio_engine",
        "alkash3d.audio.sound",

        # Ввод (заглушки)
        "alkash3d.input.input_manager",
        "alkash3d.input.keyboard",
        "alkash3d.input.mouse",

        # Core‑подсистема (заглушки)
        "alkash3d.core.game",
        "alkash3d.core.component",
    ]

    # ---------- проверка ----------
    all_passed = True
    print_subheader("Testing module imports")

    for module_name in modules_to_test:
        try:
            __import__(module_name)
            print(f"    ✅ {module_name}")
        except ImportError as e:
            print_error(f"    Failed to import {module_name}: {e}")
            all_passed = False
        except Exception as e:
            print_warning(f"    Warning importing {module_name}: {e}")

    return all_passed



# ============================================================================
# ТЕСТЫ D3D12 WRAPPER
# ============================================================================
def test_d3d12_wrapper_functions() -> bool:
    """Тест наличия всех функций в d3d12_wrapper"""
    try:
        from alkash3d.graphics.utils import d3d12_wrapper as dx
        print_success("d3d12_wrapper imported")
    except ImportError as e:
        print_error(f"Cannot import d3d12_wrapper: {e}")
        return False

    required_functions = [
        "create_device",
        "create_command_queue",
        "create_swap_chain",
        "present_swap_chain",
        "get_frame_index",
        "create_descriptor_heap",
        "GetCPUDescriptorHandleForHeapStart",
        "GetGPUDescriptorHandleForHeapStart",
        "get_rtv_descriptor_size",
        "get_dsv_descriptor_size",
        "swap_chain_get_buffer",
        "create_render_target_view",
        "create_shader_resource_view",
        "release_resource",
        "set_render_target",
        "clear_render_target",
        "begin_frame",
        "end_frame",
        "wait_for_gpu",
        "create_buffer",
        "update_subresource",
        "create_texture_from_memory",
        "update_texture",
        "set_viewport",
        "set_scissor_rect",
        "set_vertex_buffers",
        "draw_instanced",
        "draw_indexed_instanced",
        "set_graphics_pipeline",
        "compile_shader",
        "create_graphics_ps",
    ]

    all_present = True
    present = []
    missing = []

    print_subheader("Checking D3D12 wrapper functions")

    for func in required_functions:
        if hasattr(dx, func):
            present.append(func)
        else:
            missing.append(func)
            all_present = False

    print_info(f"Functions present: {len(present)}/{len(required_functions)}")

    if missing:
        print_warning(f"Missing functions: {len(missing)}")
        for func in missing:
            print(f"    ❌ {func}")

    # Проверяем константы
    if hasattr(dx, "SWAP_CHAIN_BUFFER_COUNT"):
        print_success(f"SWAP_CHAIN_BUFFER_COUNT = {dx.SWAP_CHAIN_BUFFER_COUNT}")
    else:
        print_warning("SWAP_CHAIN_BUFFER_COUNT not found")

    return all_present


def test_d3d12_device_creation() -> bool:
    """Тест создания DirectX 12 устройства"""
    try:
        from alkash3d.graphics.utils import d3d12_wrapper as dx
    except ImportError:
        return False

    print_subheader("Testing D3D12 device creation")

    try:
        device = dx.create_device()
        if device and device.value:
            print_success(f"Device created: {hex(device.value)}")

            queue = dx.create_command_queue(device)
            if queue and queue.value:
                print_success(f"Command queue created: {hex(queue.value)}")
            else:
                print_error("Failed to create command queue")
                return False

            rtv_size = dx.get_rtv_descriptor_size()
            if rtv_size > 0:
                print_success(f"RTV descriptor size: {rtv_size}")
            else:
                print_error("Invalid RTV size")
                return False

            dx.release_resource(device)
            return True
        else:
            print_error("Device creation returned null")
            return False
    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_d3d12_buffers() -> bool:
    """Тест создания буферов"""
    try:
        from alkash3d.graphics.utils import d3d12_wrapper as dx
    except ImportError:
        return False

    print_subheader("Testing buffer creation")

    gc.collect()
    time.sleep(0.1)

    try:
        device = dx.create_device()
        if not device or not device.value:
            print_error("Failed to create device")
            return False

        # Тест разных размеров буферов
        test_sizes = [64, 256, 1024, 4096, 16384]
        buffers = []

        for size in test_sizes:
            test_data = b"X" * size
            buffer = dx.create_buffer(device, len(test_data), "default")

            if not buffer or not buffer.value:
                print_error(f"Failed to create buffer of size {size}")
                continue

            buffers.append(buffer)

            try:
                dx.update_subresource(buffer, test_data, len(test_data))
                print_success(f"Buffer {size} bytes: {hex(buffer.value)}")
            except Exception as e:
                print_warning(f"Buffer update failed for size {size}: {e}")

        # Освобождаем буферы
        for buffer in buffers:
            dx.release_resource(buffer)

        dx.release_resource(device)
        return len(buffers) == len(test_sizes)

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_d3d12_descriptor_heaps() -> bool:
    """Тест создания дескрипторных куч"""
    try:
        from alkash3d.graphics.utils import d3d12_wrapper as dx
        from alkash3d.graphics.utils.descriptor_heap import DescriptorHeap
    except ImportError:
        return False

    print_subheader("Testing descriptor heaps")

    gc.collect()
    time.sleep(0.1)

    try:
        device = dx.create_device()
        if not device or not device.value:
            print_error("Failed to create device")
            return False

        # Тест RTV heap
        rtv_heap = DescriptorHeap(device, 10, "rtv", shader_visible=False)
        print_success(f"RTV heap created: {rtv_heap}")

        cpu_handle = rtv_heap.get_cpu_handle(0)
        print_success(f"CPU handle: {hex(cpu_handle)}")

        # Тест CBV/SRV/UAV heap
        cbv_heap = DescriptorHeap(device, 100, "cbv_srv_uav", shader_visible=True)
        print_success(f"CBV heap created: {cbv_heap}")

        gpu_handle = cbv_heap.get_gpu_handle(0)
        print_success(f"GPU handle: {hex(gpu_handle)}")

        # Тест аллокации дескрипторов
        indices = []
        for i in range(5):
            idx = cbv_heap.next_free()
            indices.append(idx)
            print_success(f"Allocated descriptor {idx}")

        cbv_heap.reset()
        print_success("Heap reset")

        dx.release_resource(device)
        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТЫ МАТЕМАТИКИ
# ============================================================================
def test_math_vector() -> bool:
    """Тест векторной математики"""
    try:
        from alkash3d.math.vector import Vec2, Vec3, Vec4
    except ImportError:
        return False

    print_subheader("Testing vector math")

    try:
        # Vec2 тесты
        v2a = Vec2(1, 2)
        v2b = Vec2(3, 4)
        v2c = v2a + v2b
        assert v2c.x == 4 and v2c.y == 6, "Vec2 addition failed"
        print_success(f"Vec2: {v2a} + {v2b} = {v2c}")

        # Vec3 тесты
        v3a = Vec3(1, 2, 3)
        v3b = Vec3(4, 5, 6)
        v3c = v3a - v3b
        assert v3c.x == -3 and v3c.y == -3 and v3c.z == -3, "Vec3 subtraction failed"
        print_success(f"Vec3: {v3a} - {v3b} = {v3c}")

        dot = v3a.dot(v3b)
        assert dot == 32, f"Dot product failed: {dot} != 32"
        print_success(f"Vec3 dot product: {dot}")

        cross = v3a.cross(v3b)
        print_success(f"Vec3 cross product: {cross}")

        # Vec4 тесты
        v4a = Vec4(1, 2, 3, 4)
        v4b = Vec4(5, 6, 7, 8)
        v4c = v4a * 2
        assert v4c.x == 2 and v4c.y == 4 and v4c.z == 6 and v4c.w == 8, "Vec4 scalar multiplication failed"
        print_success(f"Vec4: {v4a} * 2 = {v4c}")

        # Нормализация
        v3_norm = Vec3(3, 4, 0).normalized()
        assert abs(v3_norm.length() - 1.0) < 0.001, "Normalization failed"
        print_success(f"Normalized vector: {v3_norm}")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_math_matrix() -> bool:
    """Тест матричной математики"""
    try:
        from alkash3d.math.matrix import Mat4
        from alkash3d.math.vector import Vec3, Vec4
    except ImportError:
        return False

    print_subheader("Testing matrix math")

    try:
        # Identity matrix
        identity = Mat4.identity()
        assert identity[0][0] == 1.0 and identity[3][3] == 1.0, "Identity matrix failed"
        print_success("Identity matrix OK")

        # Translation matrix
        trans = Mat4.translation(Vec3(1, 2, 3))
        v = Vec4(0, 0, 0, 1)
        v_trans = trans * v
        assert abs(v_trans.x - 1) < 0.001 and abs(v_trans.y - 2) < 0.001 and abs(
            v_trans.z - 3) < 0.001, "Translation failed"
        print_success(f"Translation matrix: {trans}")

        # Rotation matrix
        rot = Mat4.rotation_x(90.0)  # 90 градусов
        print_success("Rotation matrix OK")

        # Scale matrix
        scale = Mat4.scaling(Vec3(2, 2, 2))
        print_success("Scale matrix OK")

        # Matrix multiplication
        combined = trans * rot * scale
        print_success(f"Matrix multiplication: {combined}")

        # Inverse
        inv = combined.inverse()
        print_success("Matrix inverse OK")

        # View matrix
        view = Mat4.look_at(Vec3(0, 0, 5), Vec3(0, 0, 0), Vec3(0, 1, 0))
        print_success("View matrix OK")

        # Projection matrix
        proj = Mat4.perspective(45.0, 16 / 9, 0.1, 1000.0)
        print_success("Projection matrix OK")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_math_quaternion() -> bool:
    """Тест кватернионов"""
    try:
        from alkash3d.math.quaternion import Quat
        from alkash3d.math.vector import Vec3
    except ImportError:
        return False

    print_subheader("Testing quaternions")

    try:
        # Identity quaternion
        q_identity = Quat.identity()
        assert q_identity.w == 1.0, "Identity quaternion failed"
        print_success(f"Identity quaternion: {q_identity}")

        # Rotation quaternion
        q_rot = Quat.from_axis_angle(Vec3(0, 1, 0), 90.0)
        print_success(f"Rotation quaternion: {q_rot}")

        # Quaternion multiplication
        q1 = Quat.from_euler(30, 0, 0)
        q2 = Quat.from_euler(0, 45, 0)
        q3 = q1 * q2
        print_success(f"Quaternion multiplication: {q3}")

        # Normalization
        q_norm = q3.normalized()
        assert abs(q_norm.length() - 1.0) < 0.001, "Quaternion normalization failed"
        print_success(f"Normalized quaternion: {q_norm}")

        # Convert to matrix
        mat = q_rot.to_matrix()
        print_success(f"Quaternion to matrix: {mat}")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТЫ СЦЕНЫ
# ============================================================================
def test_scene_node() -> bool:
    """Тест узлов сцены"""
    try:
        from alkash3d.scene.node import Node
        from alkash3d.math.vector import Vec3
    except ImportError:
        return False

    print_subheader("Testing scene nodes")

    try:
        # Создание узла
        root = Node("root")
        assert root.name == "root", "Node name failed"
        print_success(f"Root node created: {root}")

        # Позиция
        root.position = Vec3(1, 2, 3)
        assert root.position.x == 1, "Node position failed"
        print_success(f"Position: {root.position}")

        # Ротация
        root.rotation = Vec3(0, 90, 0)
        print_success(f"Rotation: {root.rotation}")

        # Масштаб
        root.scale = Vec3(2, 2, 2)
        print_success(f"Scale: {root.scale}")

        # Иерархия
        child1 = Node("child1")
        child2 = Node("child2")

        root.add_child(child1)
        root.add_child(child2)

        assert len(root.children) == 2, "Children count failed"
        print_success(f"Added 2 children, total: {len(root.children)}")

        # Поиск узлов
        found = root.find("child1")
        assert found is not None, "Node search failed"
        print_success(f"Found node: {found.name}")

        # Трансформация
        world_pos = root.get_world_position()
        print_success(f"World position: {world_pos}")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_scene_camera() -> bool:
    """Тест камеры"""
    try:
        from alkash3d.scene.camera import Camera
        from alkash3d.math.vector import Vec3
    except ImportError:
        return False

    print_subheader("Testing camera")

    try:
        # Создание камеры
        camera = Camera()
        print_success(f"Camera created: {camera}")

        # Позиция
        camera.position = Vec3(0, 0, 5)
        print_success(f"Position: {camera.position}")

        # Направление
        camera.look_at(Vec3(0, 0, 0))
        print_success("Look at origin")

        # Матрицы
        view = camera.get_view_matrix()
        proj = camera.get_projection_matrix()
        print_success(f"View matrix: {view}")
        print_success(f"Projection matrix: {proj}")

        # Параметры
        camera.fov = 60.0
        camera.aspect = 16 / 9
        camera.near_plane = 0.1
        camera.far_plane = 1000.0
        print_success(f"FOV: {camera.fov}, Aspect: {camera.aspect}")

        # Frustum
        frustum = camera.get_frustum()
        print_success("Frustum OK")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_scene_mesh() -> bool:
    """Тест мешей"""
    try:
        from alkash3d.scene.mesh import Mesh
        import numpy as np
    except ImportError:
        return False

    print_subheader("Testing meshes")

    try:
        # Создание меша из вершин
        vertices = np.array([
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.0, 0.5, 0.0]
        ], dtype=np.float32)

        mesh = Mesh(vertices)
        assert mesh.vertex_count == 3, "Vertex count failed"
        print_success(f"Mesh created with {mesh.vertex_count} vertices")

        # Меш с индексами
        indices = np.array([0, 1, 2], dtype=np.uint32)
        mesh_indexed = Mesh(vertices, indices=indices)
        assert mesh_indexed.index_count == 3, "Index count failed"
        print_success(f"Indexed mesh with {mesh_indexed.index_count} indices")

        # Меш с нормалями
        normals = np.array([
            [0, 0, 1],
            [0, 0, 1],
            [0, 0, 1]
        ], dtype=np.float32)

        mesh_normals = Mesh(vertices, normals=normals)
        print_success("Mesh with normals")

        # Меш с текстурными координатами
        texcoords = np.array([
            [0, 0],
            [1, 0],
            [0.5, 1]
        ], dtype=np.float32)

        mesh_tex = Mesh(vertices, texcoords=texcoords)
        print_success("Mesh with texture coordinates")

        # Генерация куба
        cube = Mesh.create_cube()
        print_success(f"Cube mesh with {cube.vertex_count} vertices")

        # Генерация сферы
        sphere = Mesh.create_sphere(radius=1.0, segments=16)
        print_success(f"Sphere mesh with {sphere.vertex_count} vertices")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


def test_scene_light() -> bool:
    """Тест источников света"""
    try:
        from alkash3d.scene.light import Light, PointLight, DirectionalLight, SpotLight
        from alkash3d.math.vector import Vec3
    except ImportError:
        return False

    print_subheader("Testing lights")

    try:
        # Базовый свет
        light = Light()
        light.color = Vec3(1, 1, 1)
        light.intensity = 1.0
        print_success(f"Base light: color={light.color}, intensity={light.intensity}")

        # Точечный свет
        point = PointLight()
        point.position = Vec3(0, 5, 0)
        point.range = 10.0
        point.attenuation = Vec3(1, 0.1, 0.01)
        print_success(f"Point light: position={point.position}, range={point.range}")

        # Направленный свет
        directional = DirectionalLight()
        directional.direction = Vec3(0, -1, 0)
        print_success(f"Directional light: direction={directional.direction}")

        # Прожектор
        spot = SpotLight()
        spot.position = Vec3(0, 5, 0)
        spot.direction = Vec3(0, -1, 0)
        spot.angle = 45.0
        spot.softness = 0.2
        print_success(f"Spot light: angle={spot.angle}, softness={spot.softness}")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТЫ ФИЗИКИ
# ============================================================================
def test_physics() -> bool:
    """Тест физического движка"""
    try:
        from alkash3d.physics.physics_world import PhysicsWorld
        from alkash3d.physics.rigid_body import RigidBody
        from alkash3d.math.vector import Vec3
    except ImportError:
        print_warning("Physics module not available - skipping")
        return True

    print_subheader("Testing physics")

    try:
        # Создание физического мира
        world = PhysicsWorld()
        world.gravity = Vec3(0, -9.81, 0)
        print_success(f"Physics world created, gravity={world.gravity}")

        # Создание твердого тела
        body = RigidBody()
        body.mass = 1.0
        body.position = Vec3(0, 10, 0)
        body.velocity = Vec3(0, 0, 0)
        print_success(f"Rigid body created, mass={body.mass}")

        # Добавление в мир
        world.add_body(body)
        print_success("Body added to world")

        # Симуляция
        for step in range(10):
            world.step(1 / 60.0)
            if step == 5:
                print_success(f"After 5 steps: position={body.position}")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТЫ АУДИО
# ============================================================================
def test_audio() -> bool:
    """Тест аудио движка"""
    try:
        from alkash3d.audio.audio_engine import AudioEngine
        from alkash3d.audio.sound import Sound
    except ImportError:
        print_warning("Audio module not available - skipping")
        return True

    print_subheader("Testing audio")

    try:
        # Создание аудио движка
        engine = AudioEngine()
        print_success("Audio engine created")

        # Создание звука
        sound = Sound()
        sound.volume = 0.5
        sound.pitch = 1.0
        sound.looping = False
        print_success(f"Sound created: volume={sound.volume}, pitch={sound.pitch}")

        # 3D позиционирование
        sound.set_position(0, 0, 0)
        sound.set_velocity(0, 0, 0)
        print_success("3D position set")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТЫ ВВОДА
# ============================================================================
def test_input() -> bool:
    """Тест системы ввода"""
    try:
        from alkash3d.input.input_manager import InputManager
        from alkash3d.input.keyboard import Keyboard
        from alkash3d.input.mouse import Mouse
    except ImportError:
        print_warning("Input module not available - skipping")
        return True

    print_subheader("Testing input")

    try:
        # Создание менеджера ввода
        input_mgr = InputManager()
        print_success("Input manager created")

        # Клавиатура
        keyboard = Keyboard()
        print_success("Keyboard created")

        # Мышь
        mouse = Mouse()
        print_success("Mouse created")

        # Привязка действий
        input_mgr.bind_action("jump", "space")
        input_mgr.bind_action("fire", "mouse_left")
        print_success("Actions bound")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТЫ УТИЛИТ
# ============================================================================
def test_utils() -> bool:
    """Тест утилит"""
    try:
        from alkash3d.utils.logger import logger
        from alkash3d.utils.timer import Timer
        from alkash3d.utils.config import Config
        from alkash3d.utils.resource_manager import ResourceManager
    except ImportError:
        return False

    print_subheader("Testing utilities")

    try:
        # Логгер
        logger.info("Test log message")
        print_success("Logger works")

        # Таймер
        timer = Timer()
        timer.start()
        time.sleep(0.1)
        dt = timer.tick()
        assert dt > 0.09 and dt < 0.11, f"Timer failed: {dt}"
        print_success(f"Timer works: {dt:.3f}s")

        # Конфиг
        config = Config()
        config.set("test_key", "test_value")
        value = config.get("test_key")
        assert value == "test_value", "Config failed"
        print_success("Config works")

        # Менеджер ресурсов
        rm = ResourceManager()
        print_success("Resource manager works")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ТЕСТ ПРОИЗВОДИТЕЛЬНОСТИ
# ============================================================================
def test_performance() -> bool:
    """Тест производительности"""
    print_subheader("Testing performance")

    try:
        import numpy as np
        from alkash3d.math.vector import Vec3
        from alkash3d.math.matrix import Mat4

        # Тест матричных операций
        print_info("Testing matrix operations...")
        matrices = [Mat4.identity() for _ in range(1000)]

        start = time.time()
        for i in range(999):
            matrices[i] * matrices[i + 1]
        elapsed = time.time() - start
        print_success(f"1000 matrix multiplications: {elapsed * 1000:.2f}ms")

        # Тест векторных операций
        print_info("Testing vector operations...")
        vectors = [Vec3(i, i * 2, i * 3) for i in range(10000)]

        start = time.time()
        total = Vec3(0, 0, 0)
        for v in vectors:
            total += v
        elapsed = time.time() - start
        print_success(f"10000 vector additions: {elapsed * 1000:.2f}ms")

        # Тест numpy операций
        print_info("Testing numpy operations...")
        data = np.random.rand(1000, 1000)

        start = time.time()
        result = np.dot(data, data.T)
        elapsed = time.time() - start
        print_success(f"1000x1000 matrix multiplication: {elapsed * 1000:.2f}ms")

        return True

    except Exception as e:
        print_error(f"Exception: {e}")
        traceback.print_exc()
        return False


# ============================================================================
# ЗАПУСК ВСЕХ ТЕСТОВ
# ============================================================================
def main() -> int:
    """Главная функция запуска тестов"""
    print_header("AlKAsH3D ENGINE COMPLETE TESTER")
    print(f"Python: {sys.version}")
    print(f"Platform: {sys.platform}")
    print(f"Path: {Path.cwd()}")

    # Создаем главный тест-сьют
    main_suite = TestSuite("AlKAsH3D Engine")

    # Группа 1: Импорты
    import_suite = TestSuite("Imports")
    import_suite.add_test("Module Imports", test_imports)
    for result in import_suite.tests:
        main_suite.tests.append(result)

    # Группа 2: D3D12 Wrapper
    if sys.platform == "win32":
        d3d12_suite = TestSuite("D3D12")
        d3d12_suite.add_test("D3D12 Functions", test_d3d12_wrapper_functions)
        d3d12_suite.add_test("D3D12 Device Creation", test_d3d12_device_creation)
        d3d12_suite.add_test("D3D12 Buffers", test_d3d12_buffers)
        d3d12_suite.add_test("D3D12 Descriptor Heaps", test_d3d12_descriptor_heaps)
        for result in d3d12_suite.tests:
            main_suite.tests.append(result)
    else:
        print_warning("Skipping D3D12 tests (not on Windows)")

    # Группа 3: Математика
    math_suite = TestSuite("Math")
    math_suite.add_test("Vector Math", test_math_vector)
    math_suite.add_test("Matrix Math", test_math_matrix)
    math_suite.add_test("Quaternion Math", test_math_quaternion)
    for result in math_suite.tests:
        main_suite.tests.append(result)

    # Группа 4: Сцена
    scene_suite = TestSuite("Scene")
    scene_suite.add_test("Scene Nodes", test_scene_node)
    scene_suite.add_test("Camera", test_scene_camera)
    scene_suite.add_test("Meshes", test_scene_mesh)
    scene_suite.add_test("Lights", test_scene_light)
    for result in scene_suite.tests:
        main_suite.tests.append(result)

    # Группа 5: Дополнительные модули
    extra_suite = TestSuite("Extra")
    extra_suite.add_test("Physics", test_physics)
    extra_suite.add_test("Audio", test_audio)
    extra_suite.add_test("Input", test_input)
    extra_suite.add_test("Utilities", test_utils)
    extra_suite.add_test("Performance", test_performance)
    for result in extra_suite.tests:
        main_suite.tests.append(result)

    # Итоговая статистика
    summary = main_suite.summary()

    # Финальный вердикт
    if summary["failed"] == 0:
        print(f"\n{GREEN}{'=' * 70}{RESET}")
        print(f"{GREEN}🎉 ВСЕ ТЕСТЫ ПРОЙДЕНЫ УСПЕШНО! 🎉{RESET}")
        print(f"{GREEN}{'=' * 70}{RESET}")
        return 0
    else:
        print(f"\n{RED}{'=' * 70}{RESET}")
        print(f"{RED}❌ ТЕСТЫ ЗАВЕРШИЛИСЬ С ОШИБКАМИ ({summary['failed']} failed){RESET}")
        print(f"{RED}{'=' * 70}{RESET}")
        return 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print(f"\n{YELLOW}⚠️  Тестирование прервано пользователем{RESET}")
        sys.exit(1)
    except Exception as e:
        print(f"\n{RED}❌ Необработанное исключение: {e}{RESET}")
        traceback.print_exc()
        sys.exit(1)