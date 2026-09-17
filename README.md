# 🚀 AlKAsH3D Engine

**AlKAsH3D** is a custom game engine written in Rust on top of DirectX 12: rendering, physics (a Fortran solver behind a plugin), audio (XAudio2), scripting (native Rust plugins, Lua, Python), custom file formats, and a multithreaded task scheduler. It's being built as a specialized engine for open-world simulations (currently a *My Summer Car*-style demo: a car, a garage, suspension physics) rather than as a general-purpose engine for any genre.

[![Rust](https://img.shields.io/badge/Rust-2021%2F2024%20edition-orange.svg)](https://www.rust-lang.org/)
[![DirectX](https://img.shields.io/badge/DirectX-12-blue.svg)](https://www.microsoft.com/en-us/download/details.aspx?id=104904)
[![Windows](https://img.shields.io/badge/Platform-Windows-0078D6.svg)](https://www.microsoft.com/windows)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)

---

## 📁 Repository Layout

| Folder | What it is |
|---|---|
| `alkash3d-rust/` | The engine itself — renderer, ECS scene, physics API, audio, scripting, custom file formats, demo binaries |
| `alkash3d-inertial/` | Physics plugin (`inertial.dll`) — a Fortran solver (broad/narrow phase, contacts, joints) behind a Rust FFI wrapper |
| `alkash3d-FirstFires/` | Light-culling plugin (`firstfires.dll`) — grid-based light culling |
| `alkash3d-luascript/` | Universal Lua plugin (`alkash3d_luascript.dll`) — loads and runs any `.lua` file through the shared Scripting ABI |
| `alkash3d-examplescript/` | Reference native Rust scripting plugin (`.alscript`) — a template for the script-plugin structure |
| `alkash3d-editorapp/` | Editor built on `egui`/`wgpu` — works directly with the engine's file formats via `alkash3d_rs` as an `rlib` dependency (guarantees byte-for-byte compatibility between files saved by the editor and what the engine reads) |
| `alkash3d-execfile/` | `alkash3d_execfile.exe` — a minimal standalone `.obj` viewer that drives `alkash3d_rs.dll` through the flat C-ABI (`src/capi.rs`) via `libloading`, used to sanity-check that ABI independently of the main engine binaries |

📖 Full architecture reference (subsystems, plugin ABI, all file formats, editor/execfile internals) — English and Russian — lives in [`DOCTACH.md`](DOCTACH.md).

---

## ✨ Features

### 🎮 Rendering
- **DirectX 12** — a low-level, hand-written render pipeline (no ready-made rendering framework on top of the API)
- **HDR render target** with a full transition-barrier cycle, cascaded shadow maps (CSM), volumetric lighting, a bloom pass
- **Point/spot lighting** with grid-based light culling via the FirstFires plugin
- **Frustum culling** and occlusion — a GPU occluder pass
- Async GPU readback via fences, double-buffered frames with explicit CPU/GPU synchronization

### ⚙️ Physics
- Fortran solver (`alkash3d-inertial`) — broad phase (grid), narrow phase (sphere-sphere), contacts and ball joints, atomic operations for parallel contact solving, a sleep hysteresis for inactive bodies
- Multithreaded body integration (`std::thread::scope` + non-overlapping slices)
- A separate car physics simulation (`car_physics.rs`, `car_sim.rs`) for the My Summer Car-style demo

### 🔊 Audio
- XAudio2 (built into Windows since 8 — no separate redistributable required)

### 🔌 Scripting & Plugins
- A single C-ABI (Scripting API) for script plugins — the same interface for native Rust, Lua, and Python scripts
- **Lua** (`alkash3d-luascript`, via `mlua`/Lua 5.4, vendored — no system Lua install required)
- **Python** (`rustpython-vm`, hot-reload) — pure Rust, no embeddable CPython
- Dynamic plugin loading (`.dll`) at runtime, a single shared ABI for physics/lighting/scripting

### 🗃️ Custom File Formats
| Format | Purpose |
|---|---|
| `.altex` | 3D scenes and geometry |
| `.alfar` | lighting configuration |
| `.alcar` | vehicle archives (gameplay stats: power, sound, headlights, price) |
| `.alasm` | assembly graphs — which physical parts a car/engine/gearbox is built from and how they're joined (independent of `.alcar`) |
| `.alroute` | routes and paths |
| `.alworld` | open worlds and streaming |
| `.almat` | materials and shaders |
| `.alps` | programmable shaders |
| `.alsnd` | sound systems |
| `.alscript` | scripts (native/Lua/Python) |
| `.aluv` | cinematic sequences |

### 🧩 ECS Scene Graph
- A generational-index ("sparse set") ECS in `scene.rs` — stable `EntityId`s immune to use-after-free from stale handles, parent/child hierarchy with world-transform propagation, and independently addable/removable components
- Additive to the older `Vec<MeshInstance>` API — `render_frame()` draws both, so old and new code paths can coexist during migration

### 🖌️ Editor (`alkash3d-editorapp`)
- `egui`/`wgpu`-based standalone editor: scene hierarchy, inspector, viewport with move/rotate/scale gizmos, undo/redo command history, asset browser, console
- Importers for `.obj`/`.gltf`/`.fbx`/`.blend` plus native converters/editors for every engine format (`.altex`, `.alworld`, `.alfar`, `.almat`, `.alcar`, `.alroute`, `.alsnd`, `.alscript`, `.alasm`)
- Dedicated editors: material library, sound banks, routes, scripts, car presets, assemblies
- Own animation (keyframe tracks + easing), particle system, and a small memory subsystem (object pool, frame allocator, asset cache)
- Optional Discord Rich Presence ("Editing `<scene>` — N objects")

### 🧵 Task Scheduler
- A multithreaded scheduler — heavy/light task separation, adaptive parallelization thresholds, CPU budget

---

## 🎬 Demo Binaries (`alkash3d-rust/src/bin/`)

| Binary | Command | What it shows |
|---|---|---|
| `main` | `cargo run --bin main` | Basic demo — tile walking + night lighting |
| `main1` | `cargo run --bin main1` | Solar system |
| `main2` | `cargo run --bin main2` | Simple flying cubes |
| `main_car` | `cargo run --bin main_car` | My Summer Car-style demo — ground, garage, a physics-driven car |
| `main_test` | `cargo run --bin main_test` | Verifies the player spawn point from an `.alworld` file without loading the heavy scene |
| `example_minimal` | `cargo run --bin example_minimal` | A minimal from-scratch example — for layer-by-layer GPU hang diagnostics |
| `benchmark` | `cargo run --release --bin benchmark` | FPS measurement (avg/min/max/1% low) under scalable load |

---

## 📦 Requirements

### For development
- **Rust** (stable channel; `alkash3d-rust` uses the 2024 edition, the other crates use 2021)
- **Windows 10/11**
- **DirectX 12** (built into Windows)
- **gfortran** — to build the `alkash3d-inertial` physics plugin (see its README: Linux/macOS — via your package manager, Windows — via MSYS2 + the GNU Rust target `x86_64-pc-windows-gnu`)
- **Visual Studio 2022** (C++ components — for the other plugins)

### For running
- **Windows 10/11**, a DirectX 12-capable GPU
- Plugins (optional, loaded at runtime): `inertial.dll`, `firstfires.dll`, `alkash3d_luascript.dll`

---

## 🛠️ Building

```bash
git clone <this repository's URL>
cd AlKAsH3D-Engine

# Physics plugin (needs gfortran — see alkash3d-inertial/README.md)
cd alkash3d-inertial
cargo build --release
cd ..

# The engine itself
cd alkash3d-rust
cargo build --release
cargo run --bin main_car
```

---

## ⚠️ Known Issues

- **GPU-Based Validation (GBV) is off by default.** On heavy scenes (hundreds of ECS entities, multiple shadow cascades) GBV can inflate a single frame to minutes, which looks like a driver hang to Windows and can trigger a full-system TDR recovery. It's gated behind the `ALKASH3D_GBV` env var and only takes effect for a hardcoded safe-list of light demo binaries (`main_car`, `main1`, `main2`, `physics_api_smoke`) — never for `main` (the heavy city demo).
- **An unresolved frame-2 GPU hang** (root-descriptor related, `E_INVALIDARG`) can still occur independently of GBV. If it does, `shutdown()` no longer hangs the whole machine trying to release GPU resources from a wedged device — it signals the fence with a 5-second timeout and force-exits (`std::process::exit(1)`) if the GPU doesn't respond, skipping cleanup rather than hanging in it.
- See [`DOCTACH.md`](DOCTACH.md) for full details on both subsystems.

---

## 📸 Screenshots

> *Coming soon*

---

## License

MIT — see [LICENSE](LICENSE).
