# 🚀 Alkash3D Engine

**Alkash3D** is a high-performance 3D engine written in Rust with DirectX 12 support, physics integration, dynamic plugin loading, and a multithreaded task scheduler.

[![Rust](https://img.shields.io/badge/Rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![DirectX](https://img.shields.io/badge/DirectX-12-blue.svg)](https://www.microsoft.com/en-us/download/details.aspx?id=104904)
[![Windows](https://img.shields.io/badge/Platform-Windows-0078D6.svg)](https://www.microsoft.com/windows)

---

## ✨ Features

### 🎮 Rendering
- ✅ **DirectX 12** — maximum performance
- ✅ **3D Camera** — free movement (WASD + mouse)
- ✅ **Transformations** — position, rotation, scaling
- ✅ **Instancing** — multiple mesh instances
- ✅ **Z-buffer** — proper depth testing
- ✅ **Basic Lighting** — ambient + diffuse

### ⚙️ Performance
- ✅ **Multithreaded Scheduler** — task distribution across CPU cores
- ✅ **Thread Pool** — heavy and light task separation
- ✅ **SIMD Optimizations** — via `glam` library
- ✅ **Adaptive Thresholds** — automatic parallelization tuning
- ✅ **CPU Budget** — dynamic resource management

### 🔌 Plugins
- ✅ **Dynamic Loading** — load DLL plugins at runtime
- ✅ **Physics** — physics engine integration (`inertial.dll`)
- ✅ **Light Culling** — lighting optimization (`firstfires.dll`)
- ✅ **ABI Stability** — unified API for all plugins

### 🗃️ Data Formats
- `.altex` — 3D scenes and geometry
- `.alfar` — lighting configuration
- `.alcar` — vehicle archives
- `.alroute` — routes and paths
- `.alworld` — open worlds and streaming
- `.almat` — materials and shaders
- `.alps` — programmable shaders
- `.alsnd` — sound systems
- `.alscript` — scripts (Python + Native)
- `.aluv` — cinematic sequences

---

## 📸 Screenshots

> *Screenshots coming soon*

---

## 📦 Requirements

### For Development
- **Rust** 1.70 or newer
- **Windows 10/11** (build 19042+)
- **DirectX 12** (built into Windows)
- **Visual Studio 2022** (with C++ components for plugin building)
- **RustRover** 2025.3.1+

### For Runtime
- **Windows 10/11**
- **DirectX 12 compatible GPU**
- **Plugins** (optional): `inertial.dll`, `firstfires.dll`

---

## 🛠️ Installation

### 1. Clone the Repository

```bash
git clone https://github.com/yourusername/AlKAsH3D-Engine.git
cd AlKAsH3D-Engine/alkash3d-rust
