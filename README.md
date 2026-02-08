# 📦 AlKAsH3D_engine – Python 3‑D Engine  

## Overview  

AlKAsH3D is a tiny but functional 3‑D graphics engine written completely in Python. It demonstrates how to build a modern rendering pipeline from the ground up while keeping the code base easy to read and extend. The engine provides:

* **Two rasterisation pipelines** – Forward (single‑pass) and Deferred (G‑buffer + lighting).  
* **An optional CUDA ray‑tracer** (requires an NVIDIA GPU).  
* **A scene‑graph** with hierarchical transforms, lights, and meshes.  
* **Math helpers** (`Vec3`, `Vec4`, `Mat4`, `Quat`) built on top of NumPy.  
* **Shader manager** that automatically recompiles GLSL files when they change.  
* **Simple OBJ loader**, texture loader (via Pillow), and a multithreaded task pool.  
* **Input handling** (keyboard + mouse) based on GLFW callbacks.  

The engine is deliberately lightweight – it does not try to hide OpenGL, it simply gives you the plumbing so you can focus on the graphics concepts you want to explore.

---

## Features at a glance  

| Category | What you get |
|----------|---------------|
| **Rendering** | Forward, Deferred, CUDA‑based ray tracing. |
| **Math** | 3‑D/4‑D vectors, 4×4 matrices, quaternions, all with NumPy speed. |
| **Scene‑graph** | Nodes with position, rotation (Euler), scale, parent/child hierarchy. |
| **Lights** | Directional, Point, Spot – each supplies a uniform block for shaders. |
| **Meshes** | Raw vertex data, optional normals/texcoords, indexed drawing, lazy VAO creation. |
| **Models** | Collections of meshes that behave as a single node. |
| **Shader manager** | Load GLSL from files, auto‑recompile on change, uniform helper methods. |
| **Input** | Keyboard state, mouse delta, cursor locked to the window. |
| **Utilities** | Logger (Python `logging`), OpenGL error checker, simple OBJ parser, texture loader. |
| **Multithreading** | `TaskPool` (ThreadPoolExecutor wrapper) for heavy CPU work. |

---

## Installation  

1. **Clone the repository**  

   ```text
   git clone https://github.com/TypeGuja/alkash3d.git
   cd alkash3d
   ```

2. **Install required Python packages**  

   ```text
   pip install -r requirements.txt
   ```

   The required modules are: `numpy`, `PyOpenGL`, `glfw`, `Pillow`.  
   If you plan to use the CUDA ray‑tracer, also install `numba` (requires a compatible NVIDIA driver and the CUDA Toolkit).  

3. **Platform notes**  

   * macOS – you may need to install the `glfw` library via Homebrew (`brew install glfw`).  
   * Windows – the binary wheels of `glfw` and `PyOpenGL` are usually sufficient.  

---

## Getting Started – High‑level workflow  

1. **Create a window** – the `Window` class wraps GLFW and creates an OpenGL 3.3 core context.  

2. **Instantiate the engine** – supply window size, title, and the rendering pipeline you want (`forward`, `deferred`, or `rt`).  

3. **Add a camera** – a `Camera` node is automatically placed under the scene root. Position it where you like.  

4. **Create geometry** – either build a `Mesh` from raw NumPy arrays (positions, optional normals, texcoords, indices) or load an OBJ file using the built‑in loader.  

5. **Insert geometry into the scene graph** – attach meshes or model containers as children of the root node or of any other node.  

6. **Run the engine loop** – the `Engine.run()` method handles input polling, camera movement (fly‑style), per‑frame updates, and rendering.  

7. **Shutdown** – the engine automatically destroys the window and terminates GLFW when the loop exits.  

All of the above can be achieved without writing a single line of OpenGL code; you only need to call the high‑level methods listed in the next section.

---

## Core API – What you’ll use most  

| Module / Class | Primary responsibilities | Typical usage |
|----------------|------------------------|--------------|
| `alkash3d.window.Window` | Opens a GLFW window and creates an OpenGL context. Provides width/height, an `InputManager`, and a method to swap buffers. | Create once, pass to the engine. |
| `alkash3d.engine.Engine` | Orchestrates the window, scene, camera, and chosen renderer. Offers `run()` and `shutdown()`. | Instantiate with desired pipeline, then call `run()`. |
| `alkash3d.scene.Scene` | Root `Node` of the scene graph. Holds all renderable objects and lights. Provides `update(dt)` to call per‑frame. | Add nodes (`add_child`). |
| `alkash3d.scene.Camera` | Calculates view and perspective matrices from its position and orientation. Supports fly‑style movement via `update_fly(dt, input_manager)`. | Set its `position`, optionally modify `rotation`. |
| `alkash3d.scene.Node` | Base class for anything that can be placed in the scene graph. Stores `position`, `rotation`, `scale`, parent‑child relationships, and has `get_world_matrix()`. | Subclass (e.g., `Mesh`, `Model`) or use directly for empty transform nodes. |
| `alkash3d.scene.Mesh` | Holds vertex data and lazy‑creates a VAO/VBO on first draw. Provides `draw()` and `get_model_matrix()`. | Build from NumPy arrays or link a loaded OBJ. |
| `alkash3d.scene.Model` | Simple container that groups several meshes under a single node. | Use to represent a complex object consisting of many meshes. |
| `alkash3d.scene.light.*` | Light nodes (Directional, Point, Spot) that expose a `get_uniforms()` dict for shader upload. | Add as children of the scene; the renderer automatically gathers them. |
| `alkash3d.renderer.Shader` | Loads GLSL source, compiles, links, and provides convenient `set_uniform_*` helpers. Detects source modifications and recompiles on the fly. | Obtain the shader instance from a pipeline (`engine.renderer.shader`) and set uniforms as needed. |
| `alkash3d.renderer.pipelines.ForwardRenderer` | Default forward pipeline; creates a fallback white texture and binds it to sampler 0. | Used when the engine is started with `renderer="forward"`. |
| `alkash3d.renderer.pipelines.DeferredRenderer` | G‑buffer generation followed by a lighting pass; supports up to eight lights. | Used when the engine is started with `renderer="deferred"`. |
| `alkash3d.renderer.pipelines.RTPipeline` | Thin wrapper around `RayTracer` (CUDA). | Used when the engine is started with `renderer="rt"` and a CUDA‑capable GPU is present. |
| `alkash3d.utils.loader.load_obj(path)` | Minimal OBJ parser that returns NumPy arrays for positions, normals, texcoords, and indices. | Load a model and feed the arrays into a `Mesh`. |
| `alkash3d.utils.texture_loader.load_texture(path)` | Loads an image via Pillow and creates an OpenGL texture with mip‑mapping enabled. | Load your own textures and bind them to a texture unit before drawing. |
| `alkash3d.multithread.TaskPool` | Wrapper around `ThreadPoolExecutor`; submit callables and wait for all tasks. | Use for CPU‑heavy operations such as culling or animation blending. |
| `alkash3d.core.input.InputManager` | Stores the current keyboard state and mouse movement delta; disables the OS cursor and locks it to the window. | Query keys (`is_key_pressed`) or mouse delta each frame. |
| `alkash3d.utils.logger` & `gl_check_error` | Simple logger (`logging.INFO` level) and a function that reports any pending OpenGL error. | Insert `gl_check_error("description")` after a group of OpenGL calls to catch mistakes early. |

---

## Adding Your Own Geometry  

1. **Prepare vertex data** – create NumPy `float32` arrays for positions (3 components). Optionally create arrays for normals (3) and texture coordinates (2).  

2. **Create indices** – a `uint32` NumPy array describing how vertices form triangles.  

3. **Instantiate a `Mesh`** – pass the arrays to the constructor (the order is `vertices`, optional `normals`, optional `texcoords`, optional `indices`).  

4. **Add the mesh to the scene** – call `scene.add_child(your_mesh)`.  

If you prefer not to assemble the arrays manually, use the bundled OBJ loader: call `load_obj("path/to/file.obj")`, receive the four arrays, and feed them straight into a `Mesh`.

---

## Adding Custom Shaders  

1. **Place GLSL files** – store vertex and fragment shaders under `resources/shaders/`.  

2. **Create a `Shader` instance** – give it the absolute paths to the vertex and fragment files.  

3. **Upload uniform values** – use the `set_uniform_*` methods:  
   * `set_uniform_mat4(name, matrix)` – pass a `Mat4` (the manager will call `to_gl()` automatically).  
   * `set_uniform_vec3(name, vec3)` – pass a `Vec3` or a NumPy array.  
   * `set_uniform_int/name/float` for scalar values.  

4. **Bind textures** – enable a texture unit with `glActiveTexture(GL_TEXTURE0 + unit)` and bind the texture ID. Then set the corresponding sampler uniform to the unit number (e.g., `set_uniform_int("uAlbedo", 0)`).  

5. **Hot‑reloading** – the `Shader` object monitors the modification timestamps of its source files. When you edit a GLSL file while the program runs, the next call to `shader.use()` automatically recompiles and links the updated program.  

6. **Integrate into a pipeline** – if you need a completely custom render pass, subclass `BaseRenderer` and implement `render(scene, camera)`. Inside you can use your own `Shader` instances and OpenGL state as you wish.

---

## Working with Pipelines  

* **Forward** – best for simple demos or hardware‑restricted environments. It draws every object once, applying a single material and optional texture.  

* **Deferred** – splits rendering into geometry (fills G‑buffer textures) and lighting (full‑screen quad). It enables many light sources without additional draw calls. Requires the four G‑buffer textures (position, normal, albedo+specular, depth) and a set of uniform arrays describing the lights.  

* **RT (Ray‑Tracer)** – executes a CUDA kernel that writes directly into an OpenGL texture, then blits the texture to the screen. Currently a very simple example (single sphere + background). Replace the kernel with your own algorithm for more advanced effects.  

You select the pipeline when constructing the engine (`Engine(..., renderer="forward")`). Switching pipelines at runtime is not currently supported; you must create a new `Engine` instance.

---

## Input handling  

The `InputManager` automatically registers GLFW callbacks for keyboard and mouse movement.  

* **Keyboard** – call `input_manager.is_key_pressed(glfw.KEY_X)` to query the current state.  

* **Mouse** – each frame call `input_manager.get_mouse_delta()` to obtain the relative movement since the last query. The mouse cursor is hidden and locked to the center of the window.  

The `Camera.update_fly(dt, input_manager)` method already implements a typical “fly‑through” control (WASD + mouse look). You can replace it with your own logic by subclassing `Camera` or by writing a custom `on_update` method for any node.  

---

## Debugging tips  

| Situation | What to check |
|-----------|---------------|
| **Nothing appears (gray screen)** | Verify that the view and projection matrices are uploaded with `to_gl()` (column‑major). Confirm that the viewport was set (the `Window` class now forces a viewport on creation). |
| **Object appears flat or missing texture** | Ensure texcoords are bound to attribute **location 2** (the default forward shader expects normals at location 1 and texcoords at location 2). |
| **OpenGL errors after a draw call** | Insert `gl_check_error("description")` after the suspect code block; the logger will print the error message and the location. |
| **Shader changes don’t show** | Make sure the shader files are saved on disk and that the `Shader` instance is still bound (`shader.use()`) when you render. |
| **CUDA kernel crashes** | Verify that the GPU supports CUDA and that the `numba` version matches the installed CUDA Toolkit. On machines without a compatible GPU, avoid using the `rt` renderer. |
| **Resizing the window leaves a black screen** | The engine registers a framebuffer‑size callback that forwards the new dimensions to the active renderer. If you add custom rendering code, remember to call `glViewport(0, 0, new_width, new_height)` inside your own resize handler. |

---

## Extending the engine  

* **New geometry formats** – implement another loader that returns the same four NumPy arrays (`positions, normals, texcoords, indices`).  
* **Material system** – create a node that stores texture IDs, material parameters, and a method that binds them before drawing. Extend the shader to read those uniforms.  
* **More light types** – subclass `Light`, add the needed uniform fields, and modify the deferred/forward shaders to handle them.  
* **Post‑processing** – write a new `BaseRenderer` subclass that renders the scene to an off‑screen framebuffer, then draws a full‑screen quad with a post‑process shader (e.g., tone‑mapping, bloom).  
* **Physics integration** – attach a physics body to a `Node` and update the node’s `position` and `rotation` each frame.  

All extensions can be built on top of the existing math, scene, and rendering utilities without touching the internal OpenGL boilerplate.

---

## Contributing  

1. Fork the repository.  
2. Create a feature branch (`git checkout -b feature/awesome-thing`).  
3. Implement your changes. Add unit tests for any new mathematical utilities or helper functions (the rendering code itself is best verified visually).  
4. Ensure the existing demo scripts still run on your platform.  
5. Open a Pull Request describing the problem solved or the feature added.  

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
