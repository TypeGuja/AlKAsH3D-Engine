# dx12_game_debug_fixed.py
"""
Исправленная версия с правильным рендерингом
"""

import ctypes
import ctypes.wintypes
import sys
import time
import traceback
from pathlib import Path

# Глобальный флаг трассировки
TRACE = True


def trace_print(*args, **kwargs):
    """Функция трассировки с временной меткой"""
    if TRACE:
        timestamp = time.strftime("%H:%M:%S") + f".{int(time.time() * 1000) % 1000:03d}"
        print(f"[{timestamp}]", *args, **kwargs)
        sys.stdout.flush()


print("=" * 80)
print("DX12 DEBUG GAME - FIXED VERSION")
print("=" * 80)
trace_print(f"Python version: {sys.version}")
trace_print(f"Platform: {sys.platform}")
trace_print(f"Current directory: {Path.cwd()}")

# Добавляем путь
current_dir = Path(__file__).parent
if str(current_dir) not in sys.path:
    sys.path.insert(0, str(current_dir))
    trace_print(f"Added {current_dir} to path")

# Попытка импорта
try:
    trace_print("Attempting to import d3d12_wrapper...")
    from alkash3d.graphics.utils import d3d12_wrapper as dx

    trace_print("✅ d3d12_wrapper imported successfully")

    # Проверяем наличие функций
    trace_print("\nChecking available functions:")
    functions_to_check = [
        "create_device",
        "create_command_queue",
        "create_swap_chain",
        "present_swap_chain",
        "get_frame_index",
        "create_descriptor_heap",
        "GetCPUDescriptorHandleForHeapStart",
        "get_rtv_descriptor_size",
        "swap_chain_get_buffer",
        "create_render_target_view",
        "release_resource",
        "set_render_target",
        "clear_render_target",
        "begin_frame",
        "end_frame"
    ]

    available_functions = []
    missing_functions = []

    for func in functions_to_check:
        if hasattr(dx, func):
            trace_print(f"  ✅ {func}")
            available_functions.append(func)
        else:
            trace_print(f"  ❌ {func} - MISSING!")
            missing_functions.append(func)

except Exception as e:
    trace_print(f"❌ Import failed: {e}")
    traceback.print_exc()
    sys.exit(1)

# Константы Windows
WS_OVERLAPPEDWINDOW = 0xCF0000
SW_SHOW = 5
CW_USEDEFAULT = 0x80000000

# Типы для Windows API
HINSTANCE = ctypes.c_void_p
HWND = ctypes.c_void_p
HMENU = ctypes.c_void_p
LPARAM = ctypes.c_void_p
WPARAM = ctypes.c_void_p
LRESULT = ctypes.c_void_p

user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32

trace_print("\nLoading Windows API...")
WNDPROC = ctypes.WINFUNCTYPE(LRESULT, HWND, ctypes.c_uint, WPARAM, LPARAM)


class WNDCLASSEXW(ctypes.Structure):
    _fields_ = [
        ("cbSize", ctypes.c_uint),
        ("style", ctypes.c_uint),
        ("lpfnWndProc", WNDPROC),
        ("cbClsExtra", ctypes.c_int),
        ("cbWndExtra", ctypes.c_int),
        ("hInstance", HINSTANCE),
        ("hIcon", ctypes.c_void_p),
        ("hCursor", ctypes.c_void_p),
        ("hbrBackground", ctypes.c_void_p),
        ("lpszMenuName", ctypes.c_wchar_p),
        ("lpszClassName", ctypes.c_wchar_p),
        ("hIconSm", ctypes.c_void_p),
    ]


class MSG(ctypes.Structure):
    _fields_ = [
        ("hwnd", HWND),
        ("message", ctypes.c_uint),
        ("wParam", WPARAM),
        ("lParam", LPARAM),
        ("time", ctypes.c_ulong),
        ("pt", ctypes.wintypes.POINT),
    ]


def wnd_proc(hwnd, msg, wparam, lparam):
    """Оконная процедура с трассировкой"""
    if msg == 0x0002:  # WM_DESTROY
        trace_print("  WM_DESTROY received")
        user32.PostQuitMessage(0)
        return 0
    elif msg == 0x000F:  # WM_PAINT
        ps = ctypes.create_string_buffer(64)
        user32.BeginPaint(hwnd, ctypes.byref(ps))
        user32.EndPaint(hwnd, ctypes.byref(ps))
        return 0
    elif msg == 0x0085:  # WM_NCPAINT
        return 0
    elif msg == 0x0014:  # WM_ERASEBKGND
        return 1

    # Вызываем стандартную процедуру
    try:
        DefWindowProcW = user32.DefWindowProcW
        DefWindowProcW.argtypes = [HWND, ctypes.c_uint, WPARAM, LPARAM]
        DefWindowProcW.restype = LRESULT
        return DefWindowProcW(hwnd, msg, wparam, lparam)
    except Exception:
        return 0


def create_window():
    """Создание окна с трассировкой"""
    trace_print("\n--- Creating Window ---")

    hinstance = kernel32.GetModuleHandleW(None)
    trace_print(f"hInstance: {hinstance}")

    wnd_class = WNDCLASSEXW()
    wnd_class.cbSize = ctypes.sizeof(WNDCLASSEXW)
    wnd_class.style = 0
    wnd_class.lpfnWndProc = WNDPROC(wnd_proc)
    wnd_class.cbClsExtra = 0
    wnd_class.cbWndExtra = 0
    wnd_class.hInstance = hinstance
    wnd_class.hIcon = None
    wnd_class.hCursor = None
    wnd_class.hbrBackground = 6
    wnd_class.lpszMenuName = None
    wnd_class.lpszClassName = "DX12DebugClass"
    wnd_class.hIconSm = None

    trace_print("Registering window class...")
    atom = user32.RegisterClassExW(ctypes.byref(wnd_class))
    trace_print(f"RegisterClassExW returned: {atom}")

    if atom == 0:
        error = kernel32.GetLastError()
        trace_print(f"❌ RegisterClassEx failed with error: {error}")
        raise RuntimeError(f"RegisterClassEx failed: {error}")

    trace_print("Creating window...")
    hwnd = user32.CreateWindowExW(
        0,
        wnd_class.lpszClassName,
        "DX12 Debug Game",
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT,
        CW_USEDEFAULT,
        800,
        600,
        None,
        None,
        hinstance,
        None
    )

    trace_print(f"CreateWindowExW returned: {hwnd}")

    if not hwnd:
        error = kernel32.GetLastError()
        trace_print(f"❌ CreateWindowEx failed with error: {error}")
        raise RuntimeError(f"CreateWindowEx failed: {error}")

    trace_print("Showing window...")
    user32.ShowWindow(hwnd, SW_SHOW)
    user32.UpdateWindow(hwnd)
    trace_print(f"✅ Window created: HWND={hex(int(hwnd))}")

    return hwnd, wnd_class


def process_messages():
    """Обработка сообщений с трассировкой"""
    msg = MSG()
    PeekMessageW = user32.PeekMessageW
    PeekMessageW.argtypes = [ctypes.POINTER(MSG), HWND, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    PeekMessageW.restype = ctypes.c_bool

    while PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
        if msg.message == 0x0012:  # WM_QUIT
            trace_print("WM_QUIT received")
            return False
        user32.TranslateMessage(ctypes.byref(msg))
        user32.DispatchMessageW(ctypes.byref(msg))
    return True


def test_dx12_function_calls():
    """Тестирование всех вызовов DX12 по отдельности"""
    trace_print("\n" + "=" * 60)
    trace_print("TESTING DX12 FUNCTION CALLS")
    trace_print("=" * 60)

    results = {}

    # 1. Создание устройства
    trace_print("\n1. Testing create_device()...")
    try:
        device = dx.create_device()
        results['create_device'] = bool(device and device.value)
        trace_print(f"   Device: {hex(device.value) if device else 'NULL'}")
        trace_print(f"   ✅ Success: {results['create_device']}")
    except Exception as e:
        trace_print(f"   ❌ Exception: {e}")
        traceback.print_exc()
        results['create_device'] = False
        device = None

    if not results['create_device']:
        trace_print("❌ Cannot continue without device")
        return results, None, None, 0

    # 2. Создание очереди команд
    trace_print("\n2. Testing create_command_queue()...")
    try:
        queue = dx.create_command_queue(device)
        results['create_command_queue'] = bool(queue and queue.value)
        trace_print(f"   Queue: {hex(queue.value) if queue else 'NULL'}")
        trace_print(f"   ✅ Success: {results['create_command_queue']}")
    except Exception as e:
        trace_print(f"   ❌ Exception: {e}")
        traceback.print_exc()
        results['create_command_queue'] = False
        queue = None

    # 3. Получение размера RTV
    trace_print("\n3. Testing get_rtv_descriptor_size()...")
    try:
        rtv_size = dx.get_rtv_descriptor_size()
        results['get_rtv_descriptor_size'] = rtv_size > 0
        trace_print(f"   RTV size: {rtv_size}")
        trace_print(f"   ✅ Success: {results['get_rtv_descriptor_size']}")
    except Exception as e:
        trace_print(f"   ❌ Exception: {e}")
        traceback.print_exc()
        results['get_rtv_descriptor_size'] = False
        rtv_size = 32

    return results, device, queue, rtv_size


def main():
    trace_print("\n" + "=" * 80)
    trace_print("GAME LOOP STARTING")
    trace_print("=" * 80)

    # Тестируем функции DX12
    results, device, queue, rtv_size = test_dx12_function_calls()

    if not results.get('create_device', False):
        trace_print("\n❌ CRITICAL: Device creation failed! Exiting.")
        return 1

    # Создаем окно
    try:
        hwnd, wnd_class = create_window()
    except Exception as e:
        trace_print(f"❌ Window creation failed: {e}")
        traceback.print_exc()
        return 1

    # Создаем swap chain
    trace_print("\n" + "=" * 60)
    trace_print("CREATING SWAP CHAIN")
    trace_print("=" * 60)

    try:
        trace_print(f"Calling create_swap_chain with HWND={hex(int(hwnd))}")
        swap_chain = dx.create_swap_chain(queue, int(hwnd), 800, 600)
        results['create_swap_chain'] = bool(swap_chain and swap_chain.value)
        trace_print(f"Swap chain: {hex(swap_chain.value) if swap_chain else 'NULL'}")
        trace_print(f"✅ Success: {results['create_swap_chain']}")
    except Exception as e:
        trace_print(f"❌ Exception: {e}")
        traceback.print_exc()
        results['create_swap_chain'] = False
        swap_chain = None

    if not results['create_swap_chain']:
        trace_print("❌ Cannot continue without swap chain")
        return 1

    # Создаем RTV heap
    trace_print("\n" + "=" * 60)
    trace_print("CREATING RTV HEAP")
    trace_print("=" * 60)

    try:
        trace_print("Calling create_descriptor_heap...")
        rtv_heap = dx.create_descriptor_heap(device, 2, 0, False)
        results['create_descriptor_heap'] = bool(rtv_heap and rtv_heap.value)
        trace_print(f"RTV heap: {hex(rtv_heap.value) if rtv_heap else 'NULL'}")
        trace_print(f"✅ Success: {results['create_descriptor_heap']}")
    except Exception as e:
        trace_print(f"❌ Exception: {e}")
        traceback.print_exc()
        results['create_descriptor_heap'] = False
        rtv_heap = None

    if not results['create_descriptor_heap']:
        trace_print("❌ Cannot continue without RTV heap")
        return 1

    # Получаем начальный дескриптор
    trace_print("\n" + "=" * 60)
    trace_print("GETTING DESCRIPTOR HANDLES")
    trace_print("=" * 60)

    try:
        trace_print("Calling GetCPUDescriptorHandleForHeapStart...")
        rtv_handle = dx.GetCPUDescriptorHandleForHeapStart(rtv_heap)
        results['GetCPUDescriptorHandleForHeapStart'] = rtv_handle != 0
        trace_print(f"RTV start handle: {hex(rtv_handle)}")
        trace_print(f"✅ Success: {results['GetCPUDescriptorHandleForHeapStart']}")
    except Exception as e:
        trace_print(f"❌ Exception: {e}")
        traceback.print_exc()
        results['GetCPUDescriptorHandleForHeapStart'] = False
        rtv_handle = 0

    if not results['GetCPUDescriptorHandleForHeapStart']:
        trace_print("❌ Cannot continue without RTV handle")
        return 1

    # Создаем RTV для буферов
    trace_print("\n" + "=" * 60)
    trace_print("CREATING RTVs")
    trace_print("=" * 60)

    rtv_handles = []
    for i in range(2):
        trace_print(f"\n--- Buffer {i} ---")
        try:
            trace_print(f"Calling swap_chain_get_buffer({i})...")
            buffer = dx.swap_chain_get_buffer(swap_chain, i)
            trace_print(f"Buffer {i}: {hex(buffer.value) if buffer else 'NULL'}")

            if buffer and buffer.value:
                cpu_handle = rtv_handle + (i * rtv_size)
                trace_print(f"Calling create_render_target_view at {hex(cpu_handle)}...")
                dx.create_render_target_view(device, buffer, cpu_handle)
                rtv_handles.append(cpu_handle)
                trace_print(f"✅ RTV {i} created at {hex(cpu_handle)}")
            else:
                trace_print(f"❌ Failed to get buffer {i}")
        except Exception as e:
            trace_print(f"❌ Exception for buffer {i}: {e}")
            traceback.print_exc()

    trace_print(f"\nCreated {len(rtv_handles)} RTVs")

    if len(rtv_handles) < 2:
        trace_print("❌ Not enough RTVs created")
        return 1

    # ========== ИГРОВОЙ ЦИКЛ С РЕНДЕРИНГОМ ==========
    trace_print("\n" + "=" * 80)
    trace_print("🎮 GAME LOOP WITH RENDERING 🎮")
    trace_print("=" * 80)

    frame_count = 0
    max_frames = 60
    running = True
    last_time = time.time()
    fps_counter = 0

    # Проверяем наличие функций рендеринга
    has_render_functions = all(
        f in available_functions for f in ['set_render_target', 'clear_render_target', 'begin_frame', 'end_frame'])

    if not has_render_functions:
        trace_print("\n⚠️  WARNING: Missing rendering functions!")
        trace_print("    Будет использован режим только с презентацией (без очистки)")
        trace_print("    Для полноценного рендеринга нужны функции:")
        trace_print("    - set_render_target")
        trace_print("    - clear_render_target")
        trace_print("    - begin_frame")
        trace_print("    - end_frame")
        trace_print("\n" + "=" * 80)

    while running and frame_count < max_frames:
        frame_start = time.time()

        # Обработка сообщений
        running = process_messages()

        # Получаем текущий индекс буфера
        try:
            frame_index = dx.get_frame_index()
        except Exception as e:
            trace_print(f"❌ get_frame_index error: {e}")
            frame_index = 0

        if has_render_functions:
            # ПОЛНОЦЕННЫЙ РЕНДЕРИНГ
            try:
                # Начинаем кадр
                dx.begin_frame()

                # Устанавливаем render target
                current_rtv = rtv_handles[frame_index]
                dx.set_render_target(current_rtv)

                # Создаем цвет для очистки (меняется каждый кадр)
                color = (ctypes.c_float * 4)(
                    (frame_count % 256) / 255.0,
                    ((frame_count * 2) % 256) / 255.0,
                    ((frame_count * 3) % 256) / 255.0,
                    1.0
                )

                # Очищаем render target
                dx.clear_render_target(current_rtv, color)

                # Завершаем кадр
                dx.end_frame()

                if frame_count % 10 == 0:
                    trace_print(
                        f"Frame {frame_count}: Rendered with color RGB({color[0]:.2f}, {color[1]:.2f}, {color[2]:.2f})")

            except Exception as e:
                trace_print(f"❌ Render error at frame {frame_count}: {e}")
                traceback.print_exc()
        else:
            # РЕЖИМ ТОЛЬКО ПРЕЗЕНТАЦИИ (для теста)
            try:
                dx.present_swap_chain(swap_chain, 1)
                if frame_count % 10 == 0:
                    trace_print(f"Frame {frame_count}: Presented only (no clear)")
            except Exception as e:
                trace_print(f"❌ Present error: {e}")

        frame_count += 1
        fps_counter += 1

        # FPS счетчик
        current_time = time.time()
        if current_time - last_time >= 1.0:
            trace_print(f"📊 FPS: {fps_counter}")
            fps_counter = 0
            last_time = current_time

        # Задержка для 60 FPS
        frame_time = time.time() - frame_start
        sleep_time = max(0, 0.016 - frame_time)
        if sleep_time > 0:
            time.sleep(sleep_time)

    trace_print("\n" + "=" * 80)
    trace_print(f"✅ GAME LOOP FINISHED - {frame_count} frames rendered")
    trace_print("=" * 80)

    # Очистка
    trace_print("\n" + "=" * 60)
    trace_print("CLEANING UP")
    trace_print("=" * 60)

    try:
        trace_print("Releasing swap chain...")
        dx.release_resource(swap_chain)
        trace_print("✅ Swap chain released")
    except Exception as e:
        trace_print(f"❌ Error releasing swap chain: {e}")

    try:
        trace_print("Releasing queue...")
        dx.release_resource(queue)
        trace_print("✅ Queue released")
    except Exception as e:
        trace_print(f"❌ Error releasing queue: {e}")

    try:
        trace_print("Releasing device...")
        dx.release_resource(device)
        trace_print("✅ Device released")
    except Exception as e:
        trace_print(f"❌ Error releasing device: {e}")

    trace_print("Destroying window...")
    user32.DestroyWindow(hwnd)

    # Исправляем ошибку с UnregisterClassW
    try:
        UnregisterClassW = user32.UnregisterClassW
        UnregisterClassW.argtypes = [ctypes.c_wchar_p, ctypes.c_void_p]
        UnregisterClassW.restype = ctypes.c_bool
        UnregisterClassW(wnd_class.lpszClassName, wnd_class.hInstance)
    except Exception as e:
        trace_print(f"⚠️ Warning during UnregisterClass: {e}")

    trace_print("\n" + "=" * 80)
    trace_print("TEST SUMMARY")
    trace_print("=" * 80)

    all_passed = True
    for test, passed in results.items():
        status = "✅" if passed else "❌"
        trace_print(f"{status} {test}")
        if not passed:
            all_passed = False

    trace_print("\n" + "=" * 80)
    if all_passed:
        trace_print("✅ ALL TESTS PASSED!")
        if has_render_functions:
            trace_print("\n🎉 Если вы видите меняющиеся цвета на экране - рендеринг работает!")
        else:
            trace_print("\n⚠️  Но функции рендеринга отсутствуют в DLL!")
            trace_print("\nЧтобы увидеть изображение, добавьте в Rust код функции:")
            trace_print("""
            #[no_mangle]
            pub unsafe extern "C" fn begin_frame() {{
                // реализация
            }}

            #[no_mangle] 
            pub unsafe extern "C" fn set_render_target(rtv: usize) {{
                // реализация  
            }}

            #[no_mangle]
            pub unsafe extern "C" fn clear_render_target(rtv: usize, color: *const f32) {{
                // реализация
            }}

            #[no_mangle]
            pub unsafe extern "C" fn end_frame() -> bool {{
                // реализация
                true
            }}
            """)
    else:
        trace_print("❌ SOME TESTS FAILED - проверьте ошибки выше")

    trace_print("=" * 80)

    return 0 if all_passed else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        trace_print("\n⚠️ Interrupted by user")
        sys.exit(0)
    except Exception as e:
        trace_print(f"\n❌ Unhandled exception: {e}")
        traceback.print_exc()
        sys.exit(1)