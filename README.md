# Alkash3D Engine

## Where Python meets Rust, and 3D becomes reality

---

### About The Project

**Alkash3D** is a cross-paradigm 3D engine that started its journey in Python and is now embracing the power of Rust. This repository contains the Rust-based OBJ viewer and execution environment for the Alkash3D engine.

### The Story

Once upon a time, there was a Python 3D engine. It was beautiful, it was dynamic, but it was... slow. Then came Rust - the system programming language with a borrow checker that judges your life choices. This project is the bridge between two worlds: the rapid prototyping of Python and the raw performance of Rust.

Python -> "It works on my machine!"
Rust -> "It works on EVERY machine (after fixing 47 compilation errors)"

### Current Features

**Implemented:**
- Windows GUI Window - Native Win32 window creation
- DirectX 12 Backend - Hardware-accelerated rendering via DLL
- Swap Chain & Presentation - Double-buffered rendering
- Render Target Views - Proper DX12 RTV management
- Command Queue & Lists - GPU command submission
- Clean Shutdown - Proper resource cleanup

**Work in Progress:**
- OBJ mesh loading and parsing
- 3D camera system (view/projection matrices)
- Shader pipeline (vertex/pixel shaders)
- Texture loading and sampling
- Input handling (keyboard/mouse)

**Planned:**
- Full OBJ file format support (vertices, normals, UVs, faces)
- Free-fly camera with WASD + mouse
- Multiple material support (MTL files)
- Lighting system (directional, point, spot)
- Model transformation (translation, rotation, scale)

---

### Technical Stack

- Language: Rust 2021 Edition
- Rendering: DirectX 12 (via external DLL)
- Windowing: WinAPI (raw Windows API)
- Dynamic Loading: libloading
- Build System: Cargo

---

### Building & Running

**Prerequisites:**
1. Rust toolchain (2021 edition or later)
2. Windows 10/11 with DirectX 12 support
3. alkash3d_rs.dll in project root

**Build Instructions:**
```bash
cargo build --release
copy alkash3d_rs.dll target\release\
cd target\release
.\alkash3d_viewer.exe
Expected Output:

text
=== Alkash3D OBJ Viewer ===
Window created successfully
Initializing renderer...
✓ Device created
✓ Command queue created
✓ Swap chain created
✓ RTV heap created
✓ Created 2 RTVs
✓ Command allocators created
✓ Fence created
Renderer initialized successfully!
Starting render loop...
What You'll See
Console window with initialization logs

1280x720 window titled "Alkash3D OBJ Viewer"

Dark blue screen (first rendered frame)

Current Status
The program successfully:

Creates a Windows window

Loads the DLL dynamically

Initializes DirectX 12 device and swap chain

Runs the render loop at 60 FPS

Displays a cleared screen (dark blue)

Next step: Actually rendering 3D models!

Known Issues
No actual 3D rendering yet (just clear screen)

ESC key doesn't close window (use X button)

Camera struct exists but does nothing

OBJ loader only handles cubes

License
Work in progress - details coming soon
