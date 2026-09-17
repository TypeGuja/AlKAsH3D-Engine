# AlKAsH3D Engine — Technical Documentation / Техническая документация

**[English](#english)** | **[Русский](#русский)**

---

<a id="english"></a>
# English

## 1. Overview

AlKAsH3D is a custom, from-scratch game engine written in Rust on top of raw DirectX 12 (no rendering framework on top of the API). It targets Windows and is purpose-built for open-world simulation gameplay — the flagship demo is a *My Summer Car*-style game (a car, a garage, suspension physics), not a general-purpose "make anything" engine.

The project is split into several crates/apps that all speak the engine's own binary file formats and, where applicable, a shared C-ABI plugin interface:

| Project | Role |
|---|---|
| `alkash3d-rust` | The engine core: renderer, ECS, physics bridge, audio, scripting host, file formats, demo binaries |
| `alkash3d-inertial` | Physics plugin DLL — Fortran solver behind a Rust FFI wrapper |
| `alkash3d-FirstFires` | Light-culling plugin DLL — grid-based light culling |
| `alkash3d-luascript` | Universal Lua scripting plugin DLL |
| `alkash3d-examplescript` | Reference native Rust scripting plugin, used as a template |
| `alkash3d-editorapp` | Standalone `egui`/`wgpu` scene editor, linked against `alkash3d_rs` as a Rust library (not just the C-ABI) |
| `alkash3d-execfile` | A minimal `.obj` viewer that exercises `alkash3d_rs.dll`'s flat C-ABI independently of the main engine binaries |

## 2. Engine Core Architecture (`alkash3d-rust`)

### 2.1 Module layout

The core lives mostly under `src/engine/` (the `AlkashEngine` struct and its subsystems) plus top-level modules for things that don't depend on `AlkashEngine` directly:

- `device.rs`, `queue.rs`, `swap_chain.rs`, `heap.rs`, `buffer.rs`, `texture.rs`, `pso.rs`, `shader.rs` — low-level D3D12 wrappers (device/adapter creation, command queues, swap chain, descriptor heaps, buffers/textures, pipeline state objects, shader compilation).
- `engine/window.rs` — Win32 window creation and the `WNDPROC`, resize/fullscreen handling.
- `engine/mesh.rs` — geometry primitives (`Vertex`, `Mesh`, `MeshInstance`) independent of `AlkashEngine`.
- `engine/mesh_api.rs` — API for adding meshes and spawning ECS entities.
- `engine/asset_loading.rs` — loads `.altex` geometry/textures into GPU resources, manages the material SRV heap.
- `engine/world_streaming.rs` — open-world chunk streaming: runtime chunk state, background parallel loading via `EngineScheduler`, load/unload of `.alworld` chunks.
- `engine/physics_bridge.rs` — the bridge between `AlkashEngine` and the Inertial (physics) / FirstFires (lighting) plugins, plus built-in audio hookup.
- `engine/assembly.rs` — spawns `.alasm` assemblies (a graph of parts + joints) into real physics bodies/joints/ECS entities, e.g. to let a car be disassembled part-by-part.
- `engine/day_night.rs` — day/night cycle and `ManagedLight`.
- `engine/scripting.rs`, `engine/scripting_python.rs` — script handle bookkeeping and the embedded Python (RustPython) hot-reload runtime.
- `engine/pipeline_main.rs`, `pipeline_shadow.rs`, `pipeline_post.rs`, `pipeline_occluder.rs`, `pipeline_volumetric.rs`, `pipeline_ssao.rs` — the individual render passes (see §3).
- `engine/render_frame.rs` — the main per-frame render orchestration and on-demand GPU buffer growth (`ensure_*_capacity`, see §2.3).
- `engine/lifecycle.rs` — engine startup and shutdown, including GPU-hang-safe shutdown (see §7).
- `scene.rs` — the ECS (see §2.2).
- `camera.rs`, `math.rs`, `input.rs`, `command.rs`, `render.rs`, `proc_textures.rs`, `console_log.rs`, `utils.rs` — supporting utilities (camera, vector/matrix math, input polling, a command abstraction, procedural textures, console-to-file logging).
- `audio.rs` — XAudio2 wrapper.
- `car_physics.rs`, `car_sim.rs` — a dedicated car physics/suspension simulation for the car demo, kept separate from the generic physics bridge.
- `capi.rs` — the flat C-ABI consumed by `alkash3d-execfile` (see §8).
- `plugin/` — the plugin system: ABI definitions and safe Rust wrappers (see §5).
- `scheduler/` — the task scheduler (see §6).
- `*_format.rs` — one module per custom file format (see §4).

### 2.2 ECS Scene Graph (`scene.rs`)

A simple but robust ECS using generational indices ("sparse set" style rather than a full archetype ECS) — enough for hundreds to thousands of objects, and much simpler to implement and debug than an archetype-based design.

What it gives over the older `Vec<MeshInstance>` approach:
- **Stable entity IDs.** `EntityId` is `(index, generation)`. In a plain `Vec`, removing an element from the middle shifts every index after it, invalidating anything that stored that index. Here, an entity's ID never changes for as long as it lives.
- **Use-after-free protection at the API level.** Deleting an entity bumps the `generation` of its slot. If stale code still holds the old `EntityId` and looks it up, it gets `None` — it can never silently read a different entity's data that happens to have been spawned into the same reused slot.
- **Parent/child hierarchy** with world-transform computation by traversing from the roots.
- **Independent components** that can be attached/detached individually, instead of one monolithic "everything" struct.

The ECS is strictly additive: `AlkashEngine::mesh_instances`/`meshes` keep working exactly as before, and `render_frame()` renders both the legacy instances and any ECS entities present. Old and new code can coexist, and migration can happen gradually.

### 2.3 GPU buffer growth invariant

Five `ensure_*_capacity` functions in `engine/render_frame.rs` (`ensure_constant_buffer_capacity`, `ensure_shadow_constant_buffer_capacity`, `ensure_light_buffer_capacity`, `ensure_grid_cells_buffer_capacity`, `ensure_grid_entries_buffer_capacity`) grow a GPU buffer on demand when more room is needed than was allocated. Each of them **must** call `wait_for_all_frames_idle_before_realloc()` before recreating the underlying buffer — without it, a still-in-flight frame's command list can end up referencing a buffer that was just freed and replaced, which previously caused a data-race crash. This is a load-bearing invariant, not incidental code, and it must survive any future refactor of these functions.

## 3. Rendering

- **DirectX 12**, hand-written pipeline, no third-party rendering framework.
- **HDR render target** with a full resource-transition-barrier cycle.
- **Cascaded shadow maps (CSM)** (`pipeline_shadow.rs`).
- **Volumetric lighting / god rays** (`pipeline_volumetric.rs`).
- **Bloom + tonemapping** post-process (`pipeline_post.rs`).
- **SSAO** — screen-space contact shadows (`pipeline_ssao.rs`).
- **Point/spot lighting with grid-based culling**, computed by the FirstFires plugin and consumed by the pixel shader through a light grid (each pixel only checks lights in its own grid cell instead of the whole visible list).
- **Frustum culling and GPU occlusion culling** (`pipeline_occluder.rs`).
- **Double-buffered frames** with explicit CPU/GPU synchronization via fences, and async GPU readback.

## 4. Physics

- **Broad/narrow phase, contacts, and ball joints** are implemented in Fortran (`alkash3d-inertial`, see its own `README.md` for the fix history) and reached from Rust through a C FFI wrapper.
- **Multithreaded body integration** using `std::thread::scope` with non-overlapping slices (no data races between threads).
- **Constraint/joint API**: `add_constraint`/`remove_constraint`/`get_constraint`, breakable joints reported via `get_broken_constraints` (only newly-broken joints since the last `update()`, to let the game play a sound/spawn debris exactly once per break).
- **Forces and impulses**: `apply_force`, `apply_impulse`, `apply_torque`, `apply_force_at_point` (needed for realistic suspension — spring/damper force applied at the wheel contact point rather than through the center of mass), `set_velocity`, `set_transform`.
- **Raycasts** against the physics scene via `raycast`, with an `exclude_body` option (typically used so a car's own suspension raycast doesn't hit its own chassis).
- **Static plane colliders** via `add_plane`, currently reserved for ground/floor.
- **A dedicated car physics/suspension simulation** (`car_physics.rs`, `car_sim.rs`) for the *My Summer Car*-style demo, built on top of the generic physics bridge rather than replacing it.
- **`.alasm` assemblies**: a car/engine/gearbox can be described as a tree of `PartRecord`s, each specifying which joint type binds it to its parent — letting the engine spawn a fully disassemblable vehicle (see §9, `.alasm`). This is independent of `.alcar`, which only describes gameplay stats.

## 5. Scripting & Plugin System

### 5.1 Plugin ABI

All plugins (physics, lighting, scripting) share one dynamic-loading pattern (`plugin/manager.rs`, `PluginManager`): a `.dll` is loaded at runtime, and the engine calls a well-known entry point to fetch a `#[repr(C)]` function-pointer table (`PhysicsAPI`, `LightAPI`, `ScriptingAPI` in `plugin/{physics_api,light_api,scripting_api}.rs`) plus an opaque `instance` pointer. Safe Rust wrappers (`PhysicsPlugin`, `LightPlugin`, `ScriptingPlugin` in `plugin/mod.rs`) hide the raw function-pointer calls behind ordinary methods.

Scripting is the odd one out: while there is exactly one `PhysicsPlugin` and one `LightPlugin` loaded at a time, the engine keeps a `HashMap<String, ScriptingPlugin>` — one entry per distinct scripting DLL — because a single DLL (e.g. the Lua plugin) can back many attached script instances via `create_script`/`create_script_with_source`.

### 5.2 Supported script languages

A single Scripting API abstracts over three implementations:

| # | Language | How it runs |
|---|---|---|
| 0 | **Python** | Hot-reload, embedded directly in the engine process via `rustpython-vm`/`rustpython-stdlib` (pure Rust — no system CPython install needed). `.alscript` stores the *path* to a `.py` file; the engine watches its mtime and reloads it. |
| 1 | **Lua** | Compiled/packaged as a DLL (`alkash3d-luascript`), same C-ABI as native plugins, via `mlua` against a vendored Lua 5.4 (no system Lua install needed). One DLL serves many `.lua` files via `create_script_with_source`. |
| 2 | **Native (Rust/C++)** | A DLL implementing the Scripting API directly (`alkash3d-examplescript` is the reference template); logic is baked into the DLL, so `create_script` (without a source path) is enough. |
| 3 | *(reserved)* | C# was deliberately removed from scripting (it required a .NET/CoreCLR host, i.e. installing the .NET SDK) and this slot is reserved in case it's reintroduced later; no code path currently implements it. |

## 6. Task Scheduler (`scheduler/`)

A multithreaded scheduler (`EngineScheduler`) targeting 4–8 core machines:
- `pool.rs` — worker thread pool.
- `budget.rs` — a CPU time budget the frame is allowed to spend on background work.
- `adaptive.rs` — adaptive thresholds that decide when a workload is worth parallelizing versus running inline.
- `task.rs` — the task abstraction, split between "heavy" and "light" work.
- `SchedulerStats` tracks per-frame timing (broad/narrow phase, solver, render, culling) and task counts, useful for profiling where frame time actually goes.

World streaming (`engine/world_streaming.rs`) uses this scheduler to load chunks in the background without stalling the render thread.

## 7. Engine Lifecycle & GPU-Hang Safety

`engine/lifecycle.rs` handles startup and shutdown. Shutdown signals the GPU fence and waits for it **with a 5-second timeout**. If the GPU doesn't respond in time (i.e. it's hung), the engine does **not** proceed to release GPU resources (which previously could hang indefinitely against an already-wedged device) — instead it logs the device-removed reason and force-exits the process (`std::process::exit(1)`), skipping cleanup entirely rather than risking a hang that used to be able to take down the whole machine, not just the process.

`device.rs` disables **GPU-Based Validation (GBV)** by default: on a heavy scene (hundreds of ECS entities, multiple shadow cascades) GBV can inflate a single frame's time to minutes, which Windows interprets as a driver hang and can trigger a full-system TDR (Timeout Detection and Recovery) event severe enough to freeze the machine rather than just recover the GPU. GBV can be turned on via the `ALKASH3D_GBV` environment variable for diagnostics, but even then it only takes effect for a hardcoded safe-list of light demo binaries (`main_car`, `main1`, `main2`, `physics_api_smoke`); it is unconditionally ignored for `main` (the heavy city demo) and any unrecognized binary.

The underlying GPU bug that originally motivated GBV — a frame-2 hang tied to root-descriptor contents (`E_INVALIDARG`) — is **not** itself fixed; only its consequence (the shutdown path cascading into a full machine freeze) has been addressed. Treat running unfamiliar or heavily modified engine builds with the same caution you'd give any code that talks directly to the GPU driver.

## 8. `alkash3d-execfile` — Minimal C-ABI Viewer

`alkash3d-execfile` (crate name `alkash3d_viewer`, binary `alkash3d_execfile.exe`) is a small standalone Win32 `.obj` viewer. It does **not** link against `alkash3d_rs` as a library; instead it loads `alkash3d_rs.dll` at runtime via `libloading` and calls into the flat C-ABI defined in `alkash3d-rust/src/capi.rs`.

That C-ABI intentionally mirrors an older, simpler DLL export surface (`begin_frame`, `end_frame`, `wait_for_gpu`, `get_frame_index`, `clear_render_target`, `set_viewport`, `set_scissor_rect`, `get_rtv_descriptor_size`, etc.) so that `execfile`'s existing loader code keeps working. Most of these functions ignore any device/queue pointer `execfile` passes in — the engine only ever has one global state (`crate::STATE`), not independent render contexts, so those parameters exist purely for signature compatibility. Every pointer this API hands back to `execfile` (device/queue/swap chain/heap/resource/command list) is an opaque `Box<CapiHandle>` that `execfile` only ever stores and passes back, never dereferences — so it's safe to treat them as owning wrappers.

Unlike the main engine's pipelined double-buffering, `capi.rs`'s `begin_frame`/`end_frame` intentionally do a full synchronous GPU stall at the end of every frame — the slowest possible approach, but the safest: it guarantees the current frame's allocator is never reused while the GPU might still be executing commands from it, because the previous frame is already known to be finished by then. This module never touches the main engine's `ensure_*_capacity` buffer-growth machinery (see §2.3) — it is a fully separate, simpler code path.

## 9. Custom File Formats

Every format below lives in its own `alkash3d-rust/src/<name>_format.rs` module, typically with a fixed-size `#[repr(C)]` header carrying a magic string and offsets into variable-length sections (string tables, data tables, etc.), read/written with `std::io::{Read, Write, Seek}`.

| Extension | Magic | Purpose |
|---|---|---|
| `.altex` | — | 3D scene geometry, plus texture/PBR material references |
| `.alfar` | — | Lighting configuration: ambient, global settings, individual lights, light groups, light animation |
| `.alcar` | — | Vehicle *gameplay* archive: mesh, physics, audio, lights, metadata (power, sound, headlights, price, etc.) |
| `.alasm` | — | Assembly graph: which physical parts a car/engine/gearbox is built from and which joint type binds each part to its parent (`crate::plugin::joint_type`). Independent of `.alcar` — a full vehicle can eventually reference an `.alasm` through `.alcar`'s free-form `custom_data` field, but that link is not wired up yet. |
| `.alroute` | — | Routes and waypoints (e.g. AI paths, cinematic camera paths) |
| `.alworld` | `ALKWORLD` | Open-world streaming: chunk grid, world bounds, chunk size (default 64 m), active-chunk cap, per-chunk flags (streaming/LOD/collision) |
| `.almat` | `ALKALMAT` | Materials: bucketed by render type (opaque/transparent/decal), string table, texture atlas references, author-defined materials |
| `.alps` | `ALKALPS ` | Programmable shaders: techniques, shader permutations, compiled bytecode |
| `.alsnd` | `ALKALSND` | Spatial sound banks: engine backend tag (XAudio2/WASAPI/OpenAL/Custom), channel layout, sample rate, bit depth, sound/sound-bank tables |
| `.alscript` | — | Script metadata: language (`0`=Python, `1`=Lua, `2`=Native, `3` reserved) and either a source path (Python) or a DLL reference (Lua/Native) |
| `.aluv` | `ALKALUV ` | Cinematic sequences: tracks, keyframes, camera paths, total duration |

## 10. Editor (`alkash3d-editorapp`)

A standalone `egui`/`wgpu` editor (crate `alkash3d-editor`, binary `alkash3d-editor`). Crucially, it depends on `alkash3d_rs` **as a Rust library (`rlib`)**, not just through the C-ABI (`alkash3d_rs` is built with `crate-type = ["cdylib", "rlib"]` for exactly this reason) — this gives the editor direct access to the same `*Format::save()/load()` code the engine itself uses, guaranteeing that files the editor writes are byte-for-byte compatible with what the engine reads, rather than relying on a hand-maintained mirror of each format.

Structure:
- `app.rs` — the top-level `EditorApp` state (scene, camera, active tool, panel visibility flags, undo/redo history, FPS counter, console buffer).
- `editor/` — gizmo (move/rotate/scale/select tools), 3D gizmo rendering, undo/redo command history, mesh editing.
- `scene/` — the editor's own scene graph (`GameObject`, `ObjectType`), separate from the engine's ECS but conceptually mirroring it.
- `gpu/` — the `wgpu`-based viewport renderer (camera, lights, materials, meshes, pipeline).
- `converters/` — importers (`obj.rs`, `blend.rs`, `fbx.rs`, `gltf.rs`, used for preview/intermediate representation) and exporters/importers to the engine's *native* formats (`altex.rs`, `alworld.rs`, `alfar.rs`, `almat.rs`, `alcar.rs`, `alroute.rs`, `alsnd.rs`, `alscript.rs`, `alasm.rs`) — the latter call straight into `alkash3d_rs`.
- `ui/` — dockable panels: menu bar, hierarchy, inspector, console, status bar, viewport, dialogs, asset browser, plus dedicated editors for sound banks, routes, scripts, car presets, materials, assemblies.
- `systems/` — editor-side runtime systems: spatial audio preview, cinematics, a material accelerator, shader manager, and a `WorldStreamer` (chunked streaming preview mirroring the engine's own).
- `animation/` — keyframe tracks with easing (`Interpolatable`, `AnimationTrack`).
- `particle/` — a particle system for authoring effects.
- `memory/` — a small memory subsystem (`ObjectPool`, `FrameAllocator`, `AssetCache`) used internally by the editor, not shared with the engine runtime.
- `discord_presence.rs` — optional Discord Rich Presence ("Editing `<scene>` — N objects") via IPC to the local Discord desktop client. This is unrelated to Discord's in-game overlay (which is enabled entirely from Discord's own settings and needs no code) — Rich Presence is the only "I'm currently in this program" signal that can be driven from code.

## 11. Demo Binaries (`alkash3d-rust/src/bin/`)

| Binary | Command | What it shows |
|---|---|---|
| `main` | `cargo run --bin main` | Heavy demo — tile walking + night lighting, hundreds of entities, world streaming |
| `main1` | `cargo run --bin main1` | Solar system |
| `main2` | `cargo run --bin main2` | Simple flying cubes |
| `main_car` | `cargo run --bin main_car` | *My Summer Car*-style demo — ground, garage, a physics-driven car (21 entities — a "light" scene) |
| `main_test` | `cargo run --bin main_test` | Verifies the player spawn point read from an `.alworld` file, without loading the heavy scene |
| `example_minimal` | `cargo run --bin example_minimal` | A minimal from-scratch example for layer-by-layer GPU-hang diagnostics |
| `benchmark` | `cargo run --release --bin benchmark` | FPS measurement (avg/min/max/1% low) under scalable load |
| `physics_api_smoke` | `cargo run --bin physics_api_smoke` | Smoke test of the physics plugin ABI |

## 12. Building

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

# Editor (optional)
cd ../alkash3d-editorapp
cargo run --release

# Minimal C-ABI viewer (optional)
cd ../alkash3d-execfile
cargo run --release
```

Requirements:
- **Rust** stable (`alkash3d-rust` uses the 2024 edition; other crates use 2021).
- **Windows 10/11** with a DirectX 12-capable GPU.
- **gfortran** to build `alkash3d-inertial` (Windows: MSYS2 + the `x86_64-pc-windows-gnu` Rust target — see that crate's own `README.md`).
- **Visual Studio 2022** C++ components for the other native plugins.

## 13. Known Issues

- GPU-Based Validation is off by default and only diagnostics-gated for light demo binaries (§7).
- The frame-2 root-descriptor GPU hang that GBV was originally meant to diagnose is unresolved; only its machine-freezing consequence is mitigated.
- `.alasm` assemblies are not yet linked to `.alcar` vehicles through `custom_data` — the format and spawn machinery exist, the connection is a separate, later task.
- The physics ABI has no way to *create* new bodies mid-simulation from an `.alasm` beyond initial spawn tooling in `engine/assembly.rs`; treat this area as actively evolving.

---

<a id="русский"></a>
# Русский

## 1. Обзор

AlKAsH3D — движок, написанный с нуля на Rust поверх «сырого» DirectX 12 (без стороннего рендер-фреймворка поверх API). Целевая платформа — Windows, движок специализирован под симуляцию открытого мира, а не задуман как универсальный движок «для любого жанра». Флагманская демонстрация — игра в духе *My Summer Car* (машина, гараж, физика подвески).

Проект разбит на несколько крейтов/приложений, которые работают с собственными бинарными форматами движка и, где применимо, с общим C-ABI интерфейсом плагинов:

| Проект | Роль |
|---|---|
| `alkash3d-rust` | Ядро движка: рендерер, ECS, физический мост, звук, хост скриптинга, файловые форматы, демо-бинарники |
| `alkash3d-inertial` | Физический плагин (DLL) — Fortran-солвер за Rust FFI-обёрткой |
| `alkash3d-FirstFires` | Плагин отсечения света (DLL) — culling по пространственной сетке |
| `alkash3d-luascript` | Универсальный Lua-плагин (DLL) для скриптинга |
| `alkash3d-examplescript` | Референсный нативный Rust-плагин скриптинга, используется как шаблон |
| `alkash3d-editorapp` | Отдельный редактор на `egui`/`wgpu`, линкуется с `alkash3d_rs` как с обычной Rust-библиотекой (не только через C-ABI) |
| `alkash3d-execfile` | Минимальный `.obj`-вьювер, проверяющий плоский C-ABI `alkash3d_rs.dll` независимо от основных бинарников движка |

## 2. Архитектура ядра движка (`alkash3d-rust`)

### 2.1 Структура модулей

Ядро в основном лежит в `src/engine/` (структура `AlkashEngine` и её подсистемы), плюс модули верхнего уровня для того, что не зависит от `AlkashEngine` напрямую:

- `device.rs`, `queue.rs`, `swap_chain.rs`, `heap.rs`, `buffer.rs`, `texture.rs`, `pso.rs`, `shader.rs` — низкоуровневые обёртки D3D12 (создание устройства/адаптера, командные очереди, swap chain, дескрипторные хипы, буферы/текстуры, PSO, компиляция шейдеров).
- `engine/window.rs` — создание Win32-окна, `WNDPROC`, resize/fullscreen.
- `engine/mesh.rs` — геометрические примитивы (`Vertex`, `Mesh`, `MeshInstance`), не зависящие от `AlkashEngine`.
- `engine/mesh_api.rs` — API добавления мешей и спавна ECS-сущностей.
- `engine/asset_loading.rs` — загрузка геометрии/текстур `.altex` в GPU-ресурсы, материальный SRV-хип.
- `engine/world_streaming.rs` — стриминг чанков открытого мира: рантайм-состояние чанков, параллельная фоновая загрузка через `EngineScheduler`, загрузка/выгрузка чанков `.alworld`.
- `engine/physics_bridge.rs` — мост между `AlkashEngine` и плагинами физики (Inertial) / света (FirstFires), а также встроенный звук.
- `engine/assembly.rs` — спавн `.alasm`-сборок (граф деталей + соединения) в реальные физические тела/joints/ECS-сущности — например, чтобы машину можно было разобрать на части.
- `engine/day_night.rs` — цикл дня/ночи и `ManagedLight`.
- `engine/scripting.rs`, `engine/scripting_python.rs` — учёт хендлов скриптов и встроенный hot-reload рантайм Python (RustPython).
- `engine/pipeline_main.rs`, `pipeline_shadow.rs`, `pipeline_post.rs`, `pipeline_occluder.rs`, `pipeline_volumetric.rs`, `pipeline_ssao.rs` — отдельные проходы рендера (см. §3).
- `engine/render_frame.rs` — оркестрация рендера кадра и рост GPU-буферов по требованию (`ensure_*_capacity`, см. §2.3).
- `engine/lifecycle.rs` — запуск и остановка движка, включая безопасное завершение при зависании GPU (см. §7).
- `scene.rs` — ECS (см. §2.2).
- `camera.rs`, `math.rs`, `input.rs`, `command.rs`, `render.rs`, `proc_textures.rs`, `console_log.rs`, `utils.rs` — вспомогательные утилиты (камера, векторно-матричная математика, опрос ввода, абстракция команд, процедурные текстуры, логирование консоли в файл).
- `audio.rs` — обёртка над XAudio2.
- `car_physics.rs`, `car_sim.rs` — отдельная симуляция физики/подвески машины для демо, не смешанная с общим физическим мостом.
- `capi.rs` — плоский C-ABI для `alkash3d-execfile` (см. §8).
- `plugin/` — система плагинов: описание ABI и безопасные Rust-обёртки (см. §5).
- `scheduler/` — планировщик задач (см. §6).
- `*_format.rs` — по одному модулю на каждый собственный файловый формат (см. §4).

### 2.2 ECS-граф сцены (`scene.rs`)

Простой, но надёжный ECS на генерационных индексах (стиль «sparse set», не полноценный архетипный ECS) — этого достаточно для сотен-тысяч объектов, и гораздо проще в реализации и отладке, чем архетипный ECS.

Что это даёт по сравнению со старым `Vec<MeshInstance>`:
- **Стабильные ID сущностей.** `EntityId` — это `(индекс, поколение)`. В обычном `Vec` удаление элемента из середины сдвигает все индексы после него, обесценивая всё, что хранило такой индекс. Здесь ID сущности не меняется, пока она жива.
- **Защита от use-after-free на уровне API.** Удаление сущности увеличивает `generation` её слота. Если где-то ещё лежит старый `EntityId` и по нему обращаются — вернётся `None`, а не данные чужой сущности, случайно занявшей тот же переиспользованный слот.
- **Иерархия parent/child** с вычислением мировых трансформаций обходом от корней.
- **Независимые компоненты**, которые можно добавлять/удалять по отдельности, вместо одной монолитной структуры «на всё».

ECS строго аддитивен: `AlkashEngine::mesh_instances`/`meshes` продолжают работать в точности как раньше, а `render_frame()` рендерит и старые инстансы, и ECS-сущности, если они есть. Старый и новый код могут сосуществовать, миграция может происходить постепенно.

### 2.3 Инвариант роста GPU-буферов

Пять функций `ensure_*_capacity` в `engine/render_frame.rs` (`ensure_constant_buffer_capacity`, `ensure_shadow_constant_buffer_capacity`, `ensure_light_buffer_capacity`, `ensure_grid_cells_buffer_capacity`, `ensure_grid_entries_buffer_capacity`) увеличивают GPU-буфер по требованию, когда нужно больше места, чем было выделено. Каждая из них **обязана** вызвать `wait_for_all_frames_idle_before_realloc()` перед пересозданием буфера — без этого командный список ещё выполняющегося кадра может обратиться к буферу, который только что освободили и заменили, что раньше приводило к крашу из-за гонки данных. Это несущий инвариант, а не случайный код, и он обязан пережить любой будущий рефакторинг этих функций.

## 3. Рендеринг

- **DirectX 12**, написанный вручную пайплайн, без стороннего рендер-фреймворка.
- **HDR render target** с полным циклом transition-барьеров.
- **Cascaded shadow maps (CSM)** (`pipeline_shadow.rs`).
- **Волюметрический свет / god rays** (`pipeline_volumetric.rs`).
- **Bloom + tonemapping** пост-обработка (`pipeline_post.rs`).
- **SSAO** — контактные тени в экранном пространстве (`pipeline_ssao.rs`).
- **Точечный/прожекторный свет с culling по сетке**, который считает плагин FirstFires; пиксельный шейдер получает результат через световую сетку (каждый пиксель проверяет только фонари своей ячейки, а не весь видимый список).
- **Frustum culling и GPU occlusion culling** (`pipeline_occluder.rs`).
- **Двойная буферизация кадров** с явной CPU/GPU-синхронизацией через fence и асинхронным readback с GPU.

## 4. Физика

- **Broad/narrow phase, контакты и шаровые соединения** реализованы на Fortran (`alkash3d-inertial`, история фиксов — в его собственном `README.md`) и доступны из Rust через C FFI-обёртку.
- **Многопоточная интеграция тел** через `std::thread::scope` с непересекающимися срезами (без гонок данных между потоками).
- **API соединений/joints**: `add_constraint`/`remove_constraint`/`get_constraint`, ломающиеся соединения репортятся через `get_broken_constraints` (только новые поломки с последнего `update()`, чтобы игра могла ровно один раз проиграть звук/заспавнить обломок на каждую поломку).
- **Силы и импульсы**: `apply_force`, `apply_impulse`, `apply_torque`, `apply_force_at_point` (нужна для честной подвески — сила пружины/демпфера прикладывается в точке контакта колеса, а не через центр масс), `set_velocity`, `set_transform`.
- **Raycast** по физической сцене через `raycast`, с опцией `exclude_body` (обычно используется, чтобы raycast подвески машины не попадал в собственный кузов).
- **Статичные коллайдеры-плоскости** через `add_plane`, пока зарезервированы под пол/землю.
- **Отдельная симуляция физики/подвески машины** (`car_physics.rs`, `car_sim.rs`) для демо в духе *My Summer Car*, построенная поверх общего физического моста, а не заменяющая его.
- **Сборки `.alasm`**: машину/двигатель/коробку передач можно описать деревом `PartRecord`, где каждая деталь указывает тип соединения с родителем (`crate::plugin::joint_type`) — это позволяет заспавнить полностью разбираемую машину (см. §9, `.alasm`). Формат независим от `.alcar`, который описывает только игровые характеристики.

## 5. Скриптинг и система плагинов

### 5.1 ABI плагинов

Все плагины (физика, свет, скриптинг) используют один и тот же паттерн динамической загрузки (`plugin/manager.rs`, `PluginManager`): `.dll` грузится в рантайме, движок вызывает известную точку входа, чтобы получить таблицу указателей на функции в формате `#[repr(C)]` (`PhysicsAPI`, `LightAPI`, `ScriptingAPI` в `plugin/{physics_api,light_api,scripting_api}.rs`) и непрозрачный указатель `instance`. Безопасные Rust-обёртки (`PhysicsPlugin`, `LightPlugin`, `ScriptingPlugin` в `plugin/mod.rs`) скрывают сырые вызовы по указателям за обычными методами.

Скриптинг — исключение: пока `PhysicsPlugin` и `LightPlugin` в любой момент времени по одному экземпляру, движок хранит `HashMap<String, ScriptingPlugin>` — по одной записи на каждую отдельную DLL скриптинга — потому что одна DLL (например, Lua-плагин) может обслуживать множество прикреплённых экземпляров скриптов через `create_script`/`create_script_with_source`.

### 5.2 Поддерживаемые языки скриптов

Единый Scripting API абстрагирует три реализации:

| № | Язык | Как исполняется |
|---|---|---|
| 0 | **Python** | Hot-reload, встроен прямо в процесс движка через `rustpython-vm`/`rustpython-stdlib` (чистый Rust — системный CPython не нужен). `.alscript` хранит *путь* к `.py`-файлу; движок следит за его mtime и перечитывает файл. |
| 1 | **Lua** | Компилируется/упаковывается в DLL (`alkash3d-luascript`), тот же C-ABI, что и у нативных плагинов, через `mlua` поверх вендоренного Lua 5.4 (системный Lua не нужен). Одна DLL обслуживает много `.lua`-файлов через `create_script_with_source`. |
| 2 | **Native (Rust/C++)** | DLL, реализующая Scripting API напрямую (`alkash3d-examplescript` — референсный шаблон); логика зашита в саму DLL, поэтому достаточно `create_script` (без пути к исходнику). |
| 3 | *(зарезервировано)* | C# был сознательно вырезан из скриптинга (требовал хостинга .NET/CoreCLR, то есть установки .NET SDK); слот зарезервирован на случай, если C# вернут позже — сейчас ни один код-путь его не реализует. |

## 6. Планировщик задач (`scheduler/`)

Многопоточный планировщик (`EngineScheduler`), рассчитанный на 4–8 ядер:
- `pool.rs` — пул рабочих потоков.
- `budget.rs` — бюджет процессорного времени, который кадр может потратить на фоновую работу.
- `adaptive.rs` — адаптивные пороги, решающие, когда нагрузку стоит распараллеливать, а когда выполнить последовательно.
- `task.rs` — абстракция задачи, разделение на «тяжёлые» и «лёгкие».
- `SchedulerStats` собирает тайминги по кадру (broad/narrow phase, solver, рендер, culling) и счётчики задач — полезно для профилирования того, куда реально уходит время кадра.

Стриминг мира (`engine/world_streaming.rs`) использует этот планировщик для фоновой загрузки чанков без остановки потока рендера.

## 7. Жизненный цикл движка и безопасность при зависании GPU

`engine/lifecycle.rs` отвечает за запуск и остановку. Остановка сигналит GPU fence и ждёт его **с таймаутом в 5 секунд**. Если GPU не отвечает вовремя (то есть завис), движок **не** переходит к освобождению GPU-ресурсов (что раньше могло зависнуть навсегда на уже «зависшем» устройстве) — вместо этого логируется причина device-removed и процесс принудительно завершается (`std::process::exit(1)`), полностью пропуская очистку ресурсов вместо риска зависания, которое раньше могло положить всю систему, а не только процесс.

`device.rs` по умолчанию отключает **GPU-Based Validation (GBV)**: на тяжёлой сцене (сотни ECS-сущностей, несколько каскадов теней) GBV может раздуть время одного кадра до нескольких минут, что Windows воспринимает как зависание драйвера и может спровоцировать полноценное срабатывание TDR (Timeout Detection and Recovery), достаточно серьёзное, чтобы заморозить всю машину, а не просто восстановить GPU. GBV можно включить через переменную окружения `ALKASH3D_GBV` для диагностики, но даже тогда она действует только для жёстко заданного списка лёгких демо-бинарников (`main_car`, `main1`, `main2`, `physics_api_smoke`); для `main` (тяжёлое city-демо) и для любого нераспознанного бинарника переменная безусловно игнорируется.

Сама GPU-проблема, ради диагностики которой изначально включали GBV — зависание на кадре 2, связанное с содержимым root-дескрипторов (`E_INVALIDARG`) — **не** исправлена; устранено только её следствие (каскад из пути остановки в полную заморозку машины). К запуску незнакомых или сильно изменённых сборок движка стоит относиться с той же осторожностью, что и к любому коду, напрямую работающему с драйвером GPU.

## 8. `alkash3d-execfile` — минимальный C-ABI вьювер

`alkash3d-execfile` (имя крейта `alkash3d_viewer`, бинарник `alkash3d_execfile.exe`) — небольшой отдельный Win32 `.obj`-вьювер. Он **не** линкуется с `alkash3d_rs` как с библиотекой; вместо этого он грузит `alkash3d_rs.dll` в рантайме через `libloading` и вызывает плоский C-ABI, описанный в `alkash3d-rust/src/capi.rs`.

Этот C-ABI намеренно повторяет более старую, простую поверхность экспорта DLL (`begin_frame`, `end_frame`, `wait_for_gpu`, `get_frame_index`, `clear_render_target`, `set_viewport`, `set_scissor_rect`, `get_rtv_descriptor_size` и т.д.), чтобы существующий код загрузки в `execfile` продолжал работать. Большинство этих функций игнорируют указатель device/queue, который передаёт `execfile` — у движка всегда одно глобальное состояние (`crate::STATE`), а не независимые контексты рендера, поэтому эти параметры существуют только ради совместимости сигнатур. Каждый указатель, который этот API отдаёт `execfile` (device/queue/swap chain/heap/resource/command list), — это непрозрачный `Box<CapiHandle>`, который `execfile` только хранит и передаёт обратно, никогда не разыменовывая сам — поэтому безопасно считать их владеющими обёртками.

В отличие от конвейерной двойной буферизации основного движка, `begin_frame`/`end_frame` в `capi.rs` намеренно делают полную синхронную остановку GPU в конце каждого кадра — самый медленный, но и самый безопасный вариант: он гарантирует, что аллокатор текущего кадра никогда не переиспользуется, пока GPU ещё может выполнять его команды, потому что предыдущий кадр к этому моменту уже точно завершён. Этот модуль никогда не трогает механизм роста буферов `ensure_*_capacity` основного движка (см. §2.3) — это полностью отдельный, более простой код-путь.

## 9. Собственные файловые форматы

Каждый формат ниже живёт в своём модуле `alkash3d-rust/src/<имя>_format.rs`, как правило с заголовком фиксированного размера (`#[repr(C)]`), содержащим магическую строку и смещения на секции переменной длины (таблицы строк, таблицы данных и т.д.), читается/пишется через `std::io::{Read, Write, Seek}`.

| Расширение | Магия | Назначение |
|---|---|---|
| `.altex` | — | Геометрия 3D-сцены плюс ссылки на текстуры/PBR-материалы |
| `.alfar` | — | Настройка освещения: ambient, глобальные настройки, отдельные фонари, группы фонарей, анимация света |
| `.alcar` | — | *Игровой* архив машины: меш, физика, звук, фары, метаданные (мощность, звук, фары, цена и т.д.) |
| `.alasm` | — | Граф сборки: из каких физических деталей состоит машина/двигатель/коробка и каким типом соединения (`crate::plugin::joint_type`) каждая деталь скреплена с родителем. Независим от `.alcar` — полноценная машина в будущем сможет ссылаться на `.alasm` через свободное поле `custom_data` в `.alcar`, но эта связь пока не реализована. |
| `.alroute` | — | Маршруты и путевые точки (пути ИИ, траектории камеры для катсцен) |
| `.alworld` | `ALKWORLD` | Стриминг открытого мира: сетка чанков, границы мира, размер чанка (по умолчанию 64 м), лимит активных чанков, побитовые флаги на чанк (стриминг/LOD/коллизии) |
| `.almat` | `ALKALMAT` | Материалы: группировка по типу рендера (opaque/transparent/decal), таблица строк, ссылки на атлас текстур, авторские материалы |
| `.alps` | `ALKALPS ` | Программируемые шейдеры: техники, пермутации шейдеров, скомпилированный байткод |
| `.alsnd` | `ALKALSND` | Банки пространственного звука: тег звукового бэкенда (XAudio2/WASAPI/OpenAL/Custom), раскладка каналов, частота дискретизации, битность, таблицы звуков/банков |
| `.alscript` | — | Метаданные скрипта: язык (`0`=Python, `1`=Lua, `2`=Native, `3` зарезервировано) и либо путь к исходнику (Python), либо ссылка на DLL (Lua/Native) |
| `.aluv` | `ALKALUV ` | Кинематографические сцены: дорожки, ключевые кадры, траектории камеры, общая длительность |

## 10. Редактор (`alkash3d-editorapp`)

Отдельный редактор на `egui`/`wgpu` (крейт `alkash3d-editor`, бинарник `alkash3d-editor`). Важно, что он зависит от `alkash3d_rs` **как от обычной Rust-библиотеки (`rlib`)**, а не только через C-ABI (`alkash3d_rs` собирается с `crate-type = ["cdylib", "rlib"]` именно ради этого) — это даёт редактору прямой доступ к тому же коду `*Format::save()/load()`, что использует сам движок, гарантируя побайтовую совместимость файлов, записанных редактором, с тем, что прочитает движок, вместо ручного дублирования логики каждого формата.

Структура:
- `app.rs` — верхнеуровневое состояние `EditorApp` (сцена, камера, активный инструмент, флаги видимости панелей, история undo/redo, счётчик FPS, буфер консоли).
- `editor/` — гизмо (инструменты выбора/перемещения/поворота/масштаба), 3D-рендер гизмо, история команд undo/redo, редактирование мешей.
- `scene/` — собственный граф сцены редактора (`GameObject`, `ObjectType`), отдельный от ECS движка, но концептуально его отражающий.
- `gpu/` — рендерер вьюпорта на `wgpu` (камера, свет, материалы, меши, пайплайн).
- `converters/` — импортёры (`obj.rs`, `blend.rs`, `fbx.rs`, `gltf.rs`, используются для превью/промежуточного представления) и экспортёры/импортёры в *родные* форматы движка (`altex.rs`, `alworld.rs`, `alfar.rs`, `almat.rs`, `alcar.rs`, `alroute.rs`, `alsnd.rs`, `alscript.rs`, `alasm.rs`) — последние вызывают напрямую `alkash3d_rs`.
- `ui/` — стыкуемые панели: меню, иерархия, инспектор, консоль, статус-бар, вьюпорт, диалоги, браузер ассетов, а также отдельные редакторы звуковых банков, маршрутов, скриптов, пресетов машин, материалов, сборок.
- `systems/` — рантайм-системы редактора: превью пространственного звука, кинематика, ускоритель материалов, менеджер шейдеров, `WorldStreamer` (превью чанкового стриминга, отражающее стриминг движка).
- `animation/` — дорожки ключевых кадров с easing (`Interpolatable`, `AnimationTrack`).
- `particle/` — система частиц для авторинга эффектов.
- `memory/` — небольшая подсистема памяти (`ObjectPool`, `FrameAllocator`, `AssetCache`), используется только внутри редактора, не разделяется с рантаймом движка.
- `discord_presence.rs` — опциональный Discord Rich Presence («Editing `<scene>` — N objects») через IPC к локальному десктоп-клиенту Discord. Не имеет отношения к игровому оверлею Discord (тот включается полностью настройками самого Discord и кода не требует) — Rich Presence — единственный статус «я сейчас в этой программе», который можно включить кодом.

## 11. Демо-бинарники (`alkash3d-rust/src/bin/`)

| Бинарник | Команда | Что показывает |
|---|---|---|
| `main` | `cargo run --bin main` | Тяжёлое демо — ходьба по плиткам + ночное освещение, сотни сущностей, стриминг мира |
| `main1` | `cargo run --bin main1` | Солнечная система |
| `main2` | `cargo run --bin main2` | Простые летающие кубы |
| `main_car` | `cargo run --bin main_car` | Демо в духе *My Summer Car* — грунт, гараж, физическая машина (21 сущность — «лёгкая» сцена) |
| `main_test` | `cargo run --bin main_test` | Проверка точки спавна игрока из `.alworld`, без загрузки тяжёлой сцены |
| `example_minimal` | `cargo run --bin example_minimal` | Минимальный пример «с нуля» для послойной диагностики зависаний GPU |
| `benchmark` | `cargo run --release --bin benchmark` | Замер FPS (avg/min/max/1% low) под масштабируемой нагрузкой |
| `physics_api_smoke` | `cargo run --bin physics_api_smoke` | Smoke-тест ABI физического плагина |

## 12. Сборка

```bash
git clone <URL этого репозитория>
cd AlKAsH3D-Engine

# Физический плагин (нужен gfortran — см. alkash3d-inertial/README.md)
cd alkash3d-inertial
cargo build --release
cd ..

# Сам движок
cd alkash3d-rust
cargo build --release
cargo run --bin main_car

# Редактор (опционально)
cd ../alkash3d-editorapp
cargo run --release

# Минимальный C-ABI вьювер (опционально)
cd ../alkash3d-execfile
cargo run --release
```

Требования:
- **Rust** stable (`alkash3d-rust` использует edition 2024; остальные крейты — 2021).
- **Windows 10/11** с GPU, поддерживающим DirectX 12.
- **gfortran** для сборки `alkash3d-inertial` (Windows: MSYS2 + Rust-таргет `x86_64-pc-windows-gnu` — см. README этого крейта).
- **Visual Studio 2022** (компоненты C++) для остальных нативных плагинов.

## 13. Известные ограничения

- GPU-Based Validation по умолчанию отключена и включается только для диагностики на лёгких демо-бинарниках (§7).
- Зависание GPU на кадре 2 из-за root-дескрипторов, ради диагностики которого изначально включали GBV, не устранено; смягчено только его следствие — заморозка всей машины.
- Сборки `.alasm` пока не связаны с машинами `.alcar` через `custom_data` — формат и механизм спавна существуют, а связка — отдельная, более поздняя задача.
- В физическом ABI пока нет способа создавать новые тела из `.alasm` в середине симуляции, кроме начального спавна в `engine/assembly.rs`; эта область активно развивается.
