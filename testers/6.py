import ctypes, os

d3d = ctypes.WinDLL("d3dcompiler_47.dll")
D3DCompileFromFile = d3d.D3DCompileFromFile
D3DCompileFromFile.argtypes = [
    ctypes.c_wchar_p, ctypes.c_void_p, ctypes.c_void_p,
    ctypes.c_char_p, ctypes.c_char_p,
    ctypes.c_uint, ctypes.c_uint,
    ctypes.POINTER(ctypes.c_void_p), ctypes.POINTER(ctypes.c_void_p)
]
D3DCompileFromFile.restype = ctypes.c_long  # HRESULT

blob = ctypes.c_void_p()
err  = ctypes.c_void_p()
hr = D3DCompileFromFile(
    os.path.abspath(r"../resources/shaders/forward_vert.hlsl"),
    None, None,
    b"VSMain", b"vs_5_0",
    0, 0,
    ctypes.byref(blob), ctypes.byref(err)
)
print("HR:", hex(hr), "blob:", blob.value)
