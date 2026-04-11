```markdown
# 🎨 Alkash3D Engine

## *Where Python meets Rust, and 3D becomes reality*

---

## 📖 About The Project

**Alkash3D** is a cross-paradigm 3D engine that started its journey in Python and is now embracing the power of Rust. This repository contains the Rust-based OBJ viewer and execution environment for the Alkash3D engine.

### The Story

Once upon a time, there was a Python 3D engine. It was beautiful, it was dynamic, but it was... slow. Then came Rust - the system programming language with a borrow checker that judges your life choices. This project is the bridge between two worlds: the rapid prototyping of Python and the raw performance of Rust.

```
Python → "It works on my machine!"
Rust   → "It works on EVERY machine (after fixing 47 compilation errors)"
```

---

## 🏗️ Project Structure

```
alkash3d-execfile/
├── src/
│   ├── main.rs           # Window creation & main loop
│   ├── renderer.rs       # DirectX 12 rendering (via DLL)
│   ├── obj_loader.rs     # OBJ file parser (WIP)
│   ├── camera.rs         # Camera system (WIP)
│   └── math.rs           # Linear algebra primitives
├── Cargo.toml            # Rust package manifest
├── build.rs              # Build script for DLL linking
└── alkash3d_rs.dll       # The core rendering engine
```

---

## 🚀 Current Features

### ✅ Implemented
- **Windows GUI Window** - Native Win32 window creation
- **DirectX 12 Backend** - Hardware-accelerated rendering via DLL
- **Swap Chain & Presentation** - Double-buffered rendering
- **Render Target Views** - Proper DX12 RTV management
- **Command Queue & Lists** - GPU command submission
- **Clean Shutdown** - Proper resource cleanup

### 🚧 Work in Progress
- OBJ mesh loading and parsing
- 3D camera system (view/projection matrices)
- Shader pipeline (vertex/pixel shaders)
- Texture loading and sampling
- Input handling (keyboard/mouse)
- Actual 3D model rendering (currently just a blue screen of modern art)

### 📋 Planned Features
- Full OBJ file format support (vertices, normals, UVs, faces)
- Free-fly camera with WASD + mouse
- Multiple material support (MTL files)
- Lighting system (directional, point, spot)
- Model transformation (translation, rotation, scale)
- Performance metrics overlay

---

## 🛠️ Technical Stack

| Component | Technology |
|-----------|------------|
| **Language** | Rust 2021 Edition |
| **Rendering** | DirectX 12 (via external DLL) |
| **Windowing** | WinAPI (raw Windows API) |
| **Dynamic Loading** | libloading |
| **Build System** | Cargo |

### Why WinAPI directly?
Because why use `winit` when you can suffer like it's 1995? (Actually, for maximum control and minimal dependencies)

---

## 🔧 Building & Running

### Prerequisites

1. **Rust toolchain** (2021 edition or later)
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **Windows 10/11** with DirectX 12 support

3. **alkash3d_rs.dll** - The magic engine DLL (provided separately)

### Build Instructions

```bash
# Clone the repository
git clone https://github.com/yourusername/AlkAsH3D-Engine
cd alkash3d-execfile

# Build in release mode
cargo build --release

# Copy the DLL to output folder
copy alkash3d_rs.dll target\release\

# Run!
cd target\release
.\alkash3d_viewer.exe
```

### Expected Output

```
=== Alkash3D OBJ Viewer ===
Place alkash3d_rs.dll in the same folder as this executable
Window class registered
Window created successfully
Window created and shown!
Initializing renderer...
✓ Device created
✓ Command queue created
✓ Swap chain created
✓ RTV descriptor size: 64
✓ RTV heap created
✓ Created 2 RTVs
✓ Command allocators created
✓ Command list created
✓ Fence created
Renderer initialized successfully!
Starting render loop...
```

---

## 🎯 What You'll See

When you run the program, you'll get:

1. **A console window** - Showing all the initialization logs
2. **A 1280x720 window** - With "Alkash3D OBJ Viewer" title
3. **A beautiful dark blue screen** - Your first rendered frame! (It's not much, but it's honest work)

![Screenshot Placeholder](https://via.placeholder.com/800x450/0a0a1a/4a6aff?text=Currently:+Dark+Blue+Screen+of+Modern+Art)

---

## 🐍 Python vs 🦀 Rust: The Great Debate

| Aspect | Python Version | Rust Version |
|--------|---------------|--------------|
| **Development Speed** | ⚡ Very fast | 🐢 Compiler says no |
| **Runtime Speed** | 🐌 ~60 FPS | 🚀 Unlimited FPS |
| **Memory Safety** | 😅 Garbage collector | 💪 Borrow checker |
| **Error Messages** | "Fix it yourself" | "Here's a 50-line essay about why you're wrong" |
| **Dependencies** | `pip install everything` | `cargo: "I'll just compile the universe"` |
| **Deployment** | 500 MB (includes Python) | 5 MB (just the binary) |

---

## 📝 Code Examples

### Creating a Window (The Rust Way)
```rust
unsafe {
    let hwnd = CreateWindowExW(
        0, window_class.as_ptr(), title.as_ptr(),
        WS_OVERLAPPEDWINDOW,
        CW_USEDEFAULT, CW_USEDEFAULT,
        1280, 720,
        ptr::null_mut(), ptr::null_mut(),
        instance, ptr::null_mut()
    );
    ShowWindow(hwnd, SW_SHOW);
}
```

### Loading DLL Functions Dynamically
```rust
let lib = Library::new("alkash3d_rs.dll")?;
let create_device: Symbol<CreateDeviceFn> = lib.get(b"create_device")?;
let device = create_device();
```

---

## 🐛 Known Issues

1. **No actual 3D rendering yet** - We're working on it!
2. **ESC key doesn't close the window** - Use the ❌ button for now
3. **Camera is just a struct with dreams** - It exists, but doesn't do anything
4. **OBJ loader only loads cubes** - The most important shape

---

## 🗺️ Roadmap

### Phase 1: Foundation (Current)
- ✅ Windows window creation
- ✅ DirectX 12 initialization
- ✅ Swap chain & rendering loop
- ⬜ Basic shader pipeline

### Phase 2: Core Features (Next)
- ⬜ Vertex/index buffer management
- ⬜ OBJ file parsing (full spec)
- ⬜ Camera with view/projection matrices
- ⬜ Keyboard/mouse input

### Phase 3: Visuals (Future)
- ⬜ Phong lighting model
- ⬜ Texture mapping
- ⬜ Multiple render passes
- ⬜ Shadow mapping

### Phase 4: Polish (Eventually)
- ⬜ GUI controls (imgui-rs)
- ⬜ Model transformation tools
- ⬜ Export to video
- ⬜ Scripting API

---

## 🤝 Contributing

Contributions are welcome! Especially if you:
- Know how to make the blue screen show something else
- Can explain why the borrow checker hates me
- Have a working OBJ parser
- Found the legendary `alkash3d_rs.dll` source code

---

## 📄 License

This project is licensed under "We'll figure it out later" License - see the `LICENSE` file for details (once we create it).

---

## 🙏 Acknowledgments

- **DirectX 12** - For being simultaneously powerful and painful
- **Rust Community** - For making compilation errors educational
- **The Borrow Checker** - My greatest enemy and teacher
- **Coffee** - The real MVP

---

## 📞 Contact & Support

**Project Link**: [https://github.com/yourusername/AlkAsH3D-Engine](https://github.com/TypeGuja/AlkAsH3D-Engine)

**Status**: 🟡 Active Development - Features coming soon!

---

## ⭐ Star History

If this project helps you, please give it a star! It won't fix the rendering issues, but it'll make us feel better about ourselves.

---

*"Talk is cheap. Show me the render."* 
- Linus Torvalds (probably)

---

**Made with 🦀 and ☕ (mostly ☕)**
```
