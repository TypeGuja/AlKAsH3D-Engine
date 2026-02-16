--- File: README.md ---
# 📦 AlKAsH3D Engine – Python 3‑D Engine  

## !WARNING!

<img width="1494" height="916" alt="image" src="https://github.com/user-attachments/assets/aea29422-1227-4e35-b3f5-1ec273b6451c" />
for an unknown reason, there is no image when running any of the example games


## Overview  

AlKAsH3D is a **tiny but functional** 3‑D graphics engine written primarily in Python, with a high‑performance DirectX 12 (or optional OpenGL) backend compiled from Rust.  
It demonstrates how to build a modern rendering pipeline from the ground up while keeping the code base easy to read and extend.

The engine provides:

* **Two rasterisation pipelines** – Forward (single‑pass) and Deferred (G‑buffer + lighting).  
* **An optional CUDA ray‑tracer** (requires an NVIDIA GPU).  
* **A scene‑graph** with hierarchical transforms, lights, and meshes.  
* **Math helpers** (`Vec3`, `Vec4`, `Mat4`, `Quat`) built on top of NumPy.  
* **Shader manager** that automatically recompiles GLSL/HLSL files when they change.  
* **Simple OBJ loader**, texture loader (via Pillow), and a multithreaded task pool.  
* **Input handling** (keyboard + mouse) based on GLFW callbacks.  

The engine is deliberately lightweight – it does not hide the graphics API, it simply gives you the plumbing so you can focus on the graphics concepts you want to explore.

---  

## Features at a Glance  

| Category | What you get |
|----------|---------------|
| **Rendering** | Forward, Deferred, optional CUDA‑based ray tracing. |
| **Math** | 3‑D/4‑D vectors, 4 × 4 matrices, quaternions – all powered by NumPy. |
| **Scene‑graph** | Nodes with position, rotation (Euler), scale, parent/child hierarchy. |
| **Lights** | Directional, Point, Spot – each supplies a uniform block for shaders. |
| **Meshes** | Raw vertex data, optional normals/texcoords, indexed drawing, lazy VAO creation. |
| **Models** | Collections of meshes that behave as a single node. |
| **Shader manager** | Load GLSL/HLSL from files, auto‑recompile on change, uniform helper methods. |
| **Input** | Keyboard state, mouse delta, cursor locked to the window. |
| **Utilities** | Logger (`logging`), OpenGL error checker, simple OBJ parser, texture loader. |
| **Multithreading** | `TaskPool` (wrapper around `ThreadPoolExecutor`) for heavy CPU work. |

---  

## Installation  

### 1️⃣ Clone the repository  

```bash
git clone https://github.com/yourorg/AlKAsH3D-Engine.git
cd AlKAsH3D-Engine
```

### 2️⃣ Install the required Python packages  

```bash
pip install -r requirements.txt
```

The required modules are:

* `numpy`
* `PyOpenGL`
* `glfw`
* `Pillow`

If you plan to use the optional CUDA ray‑tracer, also install `numba` (it requires a compatible NVIDIA driver and the CUDA Toolkit).

### 3️⃣ Platform notes  

| OS | Remarks |
|----|---------|
| **macOS** | You may need to install the GLFW library via Homebrew: `brew install glfw`. |
| **Windows** | The binary wheels of `glfw` and `PyOpenGL` are usually sufficient. |
| **Linux** | Ensure you have the development headers for X11/GLX (most distros provide them by default). |

---  

## Building the Native Backend  

The engine talks to DirectX 12 through a small Rust crate (`alkash3d_dx12`).  
A compiled shared library (`alkash3d_dx12.dll` on Windows, `.so` on Linux, `.dylib` on macOS) must be present in the repository root.

### One‑liner (Python) that builds **and** copies the library  

```bash
python - <<'PY'
import pathlib, platform, shutil, subprocess, sys

ROOT = pathlib.Path(__file__).resolve().parent
CRATE = ROOT / "alkash3d_dx12"

# Build in Release mode
subprocess.check_call([
    "cargo", "build", "--release",
    "--manifest-path", str(CRATE / "Cargo.toml")
])

# Choose platform‑specific suffix
suffix = {
    "Windows": ".dll",
    "Linux": ".so",
    "Darwin": ".dylib"
}[platform.system()]

# Locate the compiled library
target_dir = CRATE / "target" / "release"
lib_path = next(target_dir.glob(f"*{suffix}"))
shutil.copy2(lib_path, ROOT / lib_path.name)

print(f"✅  {lib_path.name} → {ROOT}")
PY
```

> **Optional RTX module** – If you need the CUDA‑based ray‑tracer, repeat the same snippet with `CRATE = ROOT / "alkash3d_rtx"`.

After the command finishes, you will see `alkash3d_dx12.dll` (or the platform‑appropriate counterpart) sitting next to the Python source files. The engine will now be able to load it via `ctypes`.

---  

## Getting Started – High‑Level Workflow  

1. **Create a window** – `Window` wraps GLFW and creates an OpenGL 3.3 core context (or a DX12 context under the hood).  
2. **Instantiate the engine** – specify window size, title, and the rendering pipeline you want (`forward`, `deferred`, `hybrid`, or `rt`).  
3. **Add a camera** – a `Camera` node is automatically placed under the scene root; position it wherever you like.  
4. **Create geometry** – either build a `Mesh` from raw NumPy arrays (positions, optional normals, texcoords, indices) **or** load an OBJ file using the built‑in loader.  
5. **Insert geometry into the scene graph** – attach meshes or model containers as children of the root node or any other node.  
6. **Run the engine loop** – `Engine.run()` handles input polling, camera movement (fly‑style), per‑frame updates, and rendering.  
7. **Shutdown** – the engine automatically destroys the window and terminates GLFW when the loop exits.  

All of the above can be achieved without writing a single line of OpenGL/DirectX code; you only need to call the high‑level methods listed in the next section.

---  

## Core API – What You’ll Use Most  

| Module / Class | Primary responsibilities | Typical usage |
|----------------|------------------------|---------------|
| `alkash3d.window.Window` | Opens a GLFW window, creates an OpenGL (or DX12) context, gives you width/height, an `InputManager`, and a `swap_buffers()` method. | Create once, pass to the engine. |
| `alkash3d.engine.Engine` | Orchestrates the window, scene, camera, and chosen renderer. Provides `run()` and `shutdown()`. | `engine = Engine(...); engine.run()` |
| `alkash3d.scene.Scene` | Root `Node` of the scene graph; holds all renderable objects and lights; `update(dt)` is called each frame. | `scene.add_child(mesh)` |
| `alkash3d.scene.Camera` | Computes view & projection matrices from its position & orientation; implements `update_fly(dt, input_manager)` for classic FPS‑style movement. | `camera.position = Vec3(0,0,5)` |
| `alkash3d.scene.Node` | Base class for anything placed in the scene graph; stores `position`, `rotation`, `scale`, parent‑child relationships; provides `get_world_matrix()`. | Subclass (`Mesh`, `Model`) or use directly for empty transform nodes. |
| `alkash3d.scene.Mesh` | Holds vertex data; lazily creates VAO/VBO on first draw; provides `draw(backend)` and `bounding_sphere`. | `mesh = Mesh(vertices, normals, texcoords, indices)` |
| `alkash3d.scene.Model` | Simple container that groups several meshes under a single node. | Useful for complex objects made of many parts. |
| `alkash3d.scene.light.*` | Light nodes (`DirectionalLight`, `PointLight`, `SpotLight`) that expose a `get_uniforms()` dict for shader upload. | Add as children of the scene; the renderer gathers them automatically. |
| `alkash3d.renderer.Shader` | Loads GLSL/HLSL source, compiles, links, and provides `set_uniform_*` helpers (`mat4`, `vec3`, `int`, `float`). Detects source modifications and recompiles on the fly. | `shader = Shader(vs_path, fs_path); shader.use(); shader.set_uniform_mat4("uView", view)` |
| `alkash3d.renderer.pipelines.ForwardRenderer` | Default forward pipeline; creates a fallback 1×1 white texture and binds it to sampler 0. | Used when `renderer="forward"` is passed to `Engine`. |
| `alkash3d.renderer.pipelines.DeferredRenderer` | Generates G‑buffer textures, then performs a full‑screen lighting pass; supports up to eight lights. | Used when `renderer="deferred"`. |
| `alkash3d.renderer.pipelines.HybridRenderer` | Deferred geometry + optional CUDA/OptiX ray tracing (if native `rt_core` module is available). Falls back to pure deferred if not. | Used when `renderer="hybrid"`. |
| `alkash3d.renderer.pipelines.RTXRenderer` | Thin wrapper around the Rust `alkash3d_rtx` module: renders a scene to an RGBA buffer on the GPU and copies it to a DX12 texture. | Used when `renderer="rt"` and a CUDA‑capable GPU is present. |
| `alkash3d.utils.loader.load_obj(path)` | Minimal OBJ parser that returns NumPy arrays for positions, normals, texcoords, and indices. | `verts, norms, uvs, inds = load_obj("model.obj")` |
| `alkash3d.utils.texture_loader.load_texture(path, backend)` | Loads an image via Pillow and creates a GPU texture (DX12 or OpenGL, depending on backend). | `texture = load_texture("brick.png", backend)` |
| `alkash3d.multithread.TaskPool` | Wrapper around `ThreadPoolExecutor`; submit callables and wait for completion. | Useful for CPU‑heavy tasks like nav‑mesh generation or BVH building. |
| `alkash3d.core.input.InputManager` | Stores current keyboard state and mouse delta; disables the OS cursor and locks it to the window. | `if input.is_key_pressed(glfw.KEY_W): …` |
| `alkash3d.utils.logger` & `gl_check_error` | Simple logger (`logging.INFO`) and a function that reports any pending OpenGL error. | Insert `gl_check_error("after draw")` to catch mistakes early. |

---  

## Adding Your Own Geometry  

1. **Prepare vertex data** – create NumPy `float32` arrays for positions (`Nx3`). Optionally create normals (`Nx3`) and texture coordinates (`Nx2`).  
2. **Create indices** – a `uint32` NumPy array describing how vertices form triangles.  
3. **Instantiate a `Mesh`** – pass the arrays to the constructor (`Mesh(vertices, normals, texcoords, indices)`).  
4. **Add the mesh to the scene** – `scene.add_child(your_mesh)`.  

If you prefer not to assemble the arrays manually, use the bundled OBJ loader:

```python
from alkash3d.utils.loader import load_obj
verts, norms, uvs, inds = load_obj("assets/models/teapot.obj")
mesh = Mesh(verts, norms, uvs, inds)
scene.add_child(mesh)
```

---  

## Adding Custom Shaders  

1. **Place GLSL/HLSL files** under `resources/shaders/`.  
2. **Create a `Shader` instance** with the absolute paths to the vertex and fragment files.  

   ```python
   from alkash3d.renderer import Shader
   shader = Shader(
       vertex_path="resources/shaders/custom_vert.glsl",
       fragment_path="resources/shaders/custom_frag.glsl"
   )
   ```

3. **Upload uniform values** using the helper methods:  

   * `shader.set_uniform_mat4("uModel", model_matrix)`  
   * `shader.set_uniform_vec3("uTint", Vec3(1,0,0))`  
   * `shader.set_uniform_int("uTexture", 0)`  

4. **Bind textures** – activate a texture unit (`glActiveTexture(GL_TEXTURE0 + unit)`), bind the texture ID, then set the corresponding sampler uniform.  

5. **Hot‑reloading** – the `Shader` object monitors the modification timestamps of its source files. Editing the shader on disk while the program runs will automatically trigger recompilation on the next `shader.use()`.  

6. **Integrate into a pipeline** – if you need a completely custom render pass, subclass `BaseRenderer` and implement `render(scene, camera)`. Inside you can use any shaders you like.

---  

## Working with Pipelines  

| Pipeline | When to use | Remarks |
|----------|-------------|--------|
| **Forward** | Simple demos, low‑poly scenes, or when you need minimal draw‑call overhead. | Draws each object once, applying a single material and optional texture. |
| **Deferred** | Scenes with many dynamic lights; you want lighting decoupled from geometry. | Splits rendering into a geometry pass (fills G‑buffer textures) and a full‑screen lighting pass. Requires 4 G‑buffer textures (position, normal, albedo, material) plus a depth buffer. |
| **Hybrid** | Same as Deferred but with an optional CUDA/OptiX ray‑tracer for reflections, global illumination, etc. | If the native `rt_core` module is unavailable, it automatically falls back to pure deferred rendering. |
| **RTX** | Pure GPU‑based ray tracing (CUDA) – useful for research or experimental effects. | Uses the separate Rust crate `alkash3d_rtx`. The kernel writes directly into a DX12 texture which is then displayed as a full‑screen quad. |

Select the pipeline when creating the engine:

```python
engine = Engine(
    width=1280,
    height=720,
    title="AlKAsH3D Demo",
    renderer="forward",   # forward | deferred | hybrid | rt
    backend_name="dx12"  # dx12 (default) or gl (stub)
)
```

---  

## Input Handling  

`InputManager` automatically registers GLFW callbacks for keyboard and mouse movement.

| Action | Code |
|--------|------|
| **Check if a key is pressed** | `if input.is_key_pressed(glfw.KEY_SPACE): …` |
| **Get mouse delta** (relative movement since last query) | `dx, dy = input.get_mouse_delta()` |
| **Get scroll delta** | `dx, dy = input.get_scroll_delta()` |
| **Lock & hide cursor** – done automatically by the engine. | — |

The default camera implements a classic “fly‑through” control (`WASD` for translation, mouse for yaw/pitch). You can replace it by subclassing `Camera` or by adding an `on_update(dt)` method to any node.

---  

## Debugging Tips  

| Symptom | What to check |
|---------|----------------|
| **Nothing appears (black screen)** | Verify view & projection matrices are uploaded (`to_gl()` returns column‑major), and the viewport has been set (the `Window` constructor forces a viewport). |
| **Object appears flat or texture missing** | Ensure texcoords are bound to attribute location 2 (the default forward shader expects normals at location 1 and texcoords at location 2). |
| **OpenGL errors after a draw call** | Insert `gl_check_error("description")` after the suspect block; the logger will print the error and the location. |
| **Shader changes don’t show** | Make sure the shader files are saved on disk and that the `Shader` instance is still bound (`shader.use()`) when you render. |
| **CUDA kernel crashes** | Verify that the GPU supports CUDA and that the installed `numba` version matches the CUDA Toolkit. On machines without a compatible GPU, avoid the `rt` renderer. |
| **Resizing the window leaves a black screen** | The engine registers a framebuffer‑size callback that forwards new dimensions to the active renderer. If you add custom rendering code, remember to call `glViewport(0, 0, new_width, new_height)` inside your own resize handler. |

---  

## Extending the Engine  

* **New geometry formats** – implement a loader that returns the same four NumPy arrays (`positions, normals, texcoords, indices`).  
* **Material system** – create a node that stores texture IDs, material parameters, and a method that binds them before drawing. Extend the shader to read those uniforms.  
* **More light types** – subclass `Light`, add the needed uniform fields, and modify the forward/deferred shaders to handle them.  
* **Post‑processing** – write a new `BaseRenderer` subclass that renders the scene to an off‑screen framebuffer, then draws a full‑screen quad with a post‑process shader (e.g., tone‑mapping, bloom).  
* **Physics integration** – attach a physics body to a `Node` and update the node’s `position`/`rotation` each frame.  

All extensions can be built on top of the existing math, scene, and rendering utilities without touching the internal OpenGL/DirectX boilerplate.

---  

## Contributing  

1. **Fork** the repository.  
2. Create a feature branch: `git checkout -b feature/awesome-thing`.  
3. Implement your changes. Add unit tests for any new mathematical utilities or helper functions (the rendering code itself is best verified visually).  
4. Ensure the existing demo scripts still run on your platform.  
5. Open a **Pull Request** describing the problem solved or the feature added.  

Style follows **PEP‑8**, type hints are encouraged, and doc‑strings should be in English.  

---  

## License  

AlKAsH3D is released under the **MIT License**. See the `LICENSE` file for the full text.  

---  

## Acknowledgements  

* **glfw** – cross‑platform window and input handling.  
* **PyOpenGL** – Python bindings for the OpenGL API.  
* **Numba** – JIT compilation for the optional CUDA ray‑tracer.  
* **Pillow** – image loading for texture creation.  

---  

**Happy coding!** 🚀  

*The AlKAsH3D team*
