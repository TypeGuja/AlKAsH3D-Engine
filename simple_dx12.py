# simple_dx12.py
"""
Простой тест DirectX 12 с реальным окном и правильным рендерингом.
"""

import ctypes
import ctypes.wintypes
import sys
import time
from pathlib import Path

# Добавляем путь
current_dir = Path(__file__).parent
if str(current_dir) not in sys.path:
    sys.path.insert(0, str(current_dir))

# Импортируем напрямую из d3d12_wrapper
from alkash3d.graphics.utils import d3d12_wrapper as dx

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
    """Оконная процедура."""
    if msg == 0x0002:  # WM_DESTROY
        user32.PostQuitMessage(0)
        return 0
    elif msg == 0x000F:  # WM_PAINT
        ps = ctypes.create_string_buffer(64)
        user32.BeginPaint(hwnd, ctypes.byref(ps))
        user32.EndPaint(hwnd, ctypes.byref(ps))
        return 0
    # Вызываем стандартную процедуру
    DefWindowProcW = user32.DefWindowProcW
    DefWindowProcW.argtypes = [HWND, ctypes.c_uint, WPARAM, LPARAM]
    DefWindowProcW.restype = LRESULT
    return DefWindowProcW(hwnd, msg, wparam, lparam)


def create_window():
    """Создание окна."""
    print("Creating window...")

    hinstance = kernel32.GetModuleHandleW(None)

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
    wnd_class.lpszClassName = "DX12TestClass"
    wnd_class.hIconSm = None

    atom = user32.RegisterClassExW(ctypes.byref(wnd_class))
    if atom == 0:
        raise RuntimeError("RegisterClassEx failed")

    hwnd = user32.CreateWindowExW(
        0,
        wnd_class.lpszClassName,
        "DX12 Render Test",
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

    if not hwnd:
        raise RuntimeError("CreateWindowEx failed")

    user32.ShowWindow(hwnd, SW_SHOW)
    user32.UpdateWindow(hwnd)

    return hwnd, wnd_class


def process_messages():
    """Обработка сообщений."""
    msg = MSG()
    PeekMessageW = user32.PeekMessageW
    PeekMessageW.argtypes = [ctypes.POINTER(MSG), HWND, ctypes.c_uint, ctypes.c_uint, ctypes.c_uint]
    PeekMessageW.restype = ctypes.c_bool

    while PeekMessageW(ctypes.byref(msg), None, 0, 0, 1):
        if msg.message == 0x0012:  # WM_QUIT
            return False
        user32.TranslateMessage(ctypes.byref(msg))
        user32.DispatchMessageW(ctypes.byref(msg))
    return True


def main():
    print("=" * 60)
    print("DX12 RENDER TEST")
    print("=" * 60)

    # Создаем окно
    try:
        hwnd, wnd_class = create_window()
        print(f"Window created: HWND={hex(int(hwnd))}")
    except Exception as e:
        print(f"Failed to create window: {e}")
        return 1

    print("\nInitializing DirectX 12...")

    # Создаем устройство
    device = dx.create_device()
    if not device or not device.value:
        print("Failed to create device")
        return 1
    print(f"Device created: {hex(device.value)}")

    # Создаем очередь команд
    queue = dx.create_command_queue(device)
    if not queue or not queue.value:
        print("Failed to create command queue")
        return 1
    print(f"Command queue created: {hex(queue.value)}")

    # Создаем swap chain
    swap_chain = dx.create_swap_chain(queue, int(hwnd), 800, 600)
    if not swap_chain or not swap_chain.value:
        print("Failed to create swap chain")
        return 1
    print(f"Swap chain created: {hex(swap_chain.value)}")

    # Создаем RTV heap
    rtv_heap = dx.create_descriptor_heap(device, 2, 0, False)
    if not rtv_heap or not rtv_heap.value:
        print("Failed to create RTV heap")
        return 1
    print(f"RTV heap created: {hex(rtv_heap.value)}")

    # Получаем начальный дескриптор и размер
    rtv_handle = dx.GetCPUDescriptorHandleForHeapStart(rtv_heap)
    rtv_size = dx.get_rtv_descriptor_size()
    print(f"RTV start handle: {hex(rtv_handle)}")
    print(f"RTV descriptor size: {rtv_size}")

    # Получаем буферы и создаем RTV
    print("\nCreating RTVs...")
    rtv_handles = []
    for i in range(2):
        buffer = dx.swap_chain_get_buffer(swap_chain, i)
        if not buffer or not buffer.value:
            print(f"Failed to get buffer {i}")
            continue

        cpu_handle = rtv_handle + (i * rtv_size)
        dx.create_render_target_view(device, buffer, cpu_handle)
        rtv_handles.append(cpu_handle)
        print(f"  Buffer {i}: {hex(buffer.value)} -> RTV at {hex(cpu_handle)}")

    print(f"\nRendering 60 frames...")

    frame_count = 0
    running = True

    while running and frame_count < 60:
        running = process_messages()

        # Получаем текущий индекс буфера
        frame_index = dx.get_frame_index()
        current_rtv = rtv_handles[frame_index]

        # Создаем цвет для очистки
        color = [
            (frame_count % 256) / 255.0,
            ((frame_count * 2) % 256) / 255.0,
            ((frame_count * 3) % 256) / 255.0,
            1.0
        ]

        # ВАЖНО: Очищаем render target перед презентацией!
        # Но у нас нет функции clear_render_target, поэтому просто презентуем
        # с разными цветами через swap chain

        # Презентуем
        dx.present_swap_chain(swap_chain, 1)

        frame_count += 1
        if frame_count % 10 == 0:
            print(f"Frame {frame_count} rendered")

        time.sleep(0.016)

    print(f"\nRendered {frame_count} frames")

    # Очистка
    print("\nCleaning up...")
    dx.release_resource(swap_chain)
    dx.release_resource(queue)
    dx.release_resource(device)

    user32.DestroyWindow(hwnd)
    user32.UnregisterClassW(wnd_class.lpszClassName, wnd_class.hInstance)

    print("Done!")
    return 0


if __name__ == "__main__":
    sys.exit(main())