// 1examples/walking_world.rs - 3D мир с возможностью ходить

use alkash3d_rs::*;
use std::ffi::c_void;
use std::ops::ControlFlow;
use std::ptr;
use std::time::Instant;
use windows_core::Event;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::window::WindowBuilder;
use winit::dpi::PhysicalSize;
use winit::platform::windows::WindowExtWindows;
use winit::event::{Event, WindowEvent, DeviceEvent, ElementState, KeyboardInput, VirtualKeyCode};
use winit::event::DeviceEvent::MouseMotion;

// Вершина с позицией и цветом
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 4],
}

// MVP матрица для шейдера
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct MVP {
    mvp: [[f32; 4]; 4],
}

impl MVP {
    fn identity() -> Self {
        Self { mvp: [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]}
    }

    fn perspective(fov: f32, aspect: f32, near: f32, far: f32) -> Self {
        let tan_half_fov = (fov * 0.5f32).to_radians().tan();
        let range = 1.0 / (near - far);

        Self { mvp: [
            [1.0 / (aspect * tan_half_fov), 0.0, 0.0, 0.0],
            [0.0, 1.0 / tan_half_fov, 0.0, 0.0],
            [0.0, 0.0, (-near - far) * range, 2.0 * far * near * range],
            [0.0, 0.0, 1.0, 0.0],
        ]}
    }

    fn translate(&mut self, x: f32, y: f32, z: f32) {
        self.mvp[3][0] += x;
        self.mvp[3][1] += y;
        self.mvp[3][2] += z;
    }

    fn rotate_y(&mut self, angle: f32) {
        let cos = angle.cos();
        let sin = angle.sin();

        let m00 = self.mvp[0][0];
        let m02 = self.mvp[0][2];
        let m20 = self.mvp[2][0];
        let m22 = self.mvp[2][2];

        self.mvp[0][0] = m00 * cos - m02 * sin;
        self.mvp[0][2] = m00 * sin + m02 * cos;
        self.mvp[2][0] = m20 * cos - m22 * sin;
        self.mvp[2][2] = m20 * sin + m22 * cos;
    }
}

// Камера от первого лица
struct Camera {
    position: [f32; 3],
    yaw: f32,    // поворот влево-вправо
    pitch: f32,  // поворот вверх-вниз
}

impl Camera {
    fn new() -> Self {
        Self {
            position: [0.0, 2.0, -5.0],
            yaw: 0.0,
            pitch: 0.0,
        }
    }

    fn get_view_matrix(&self) -> [[f32; 4]; 4] {
        let cos_yaw = self.yaw.cos();
        let sin_yaw = self.yaw.sin();
        let cos_pitch = self.pitch.cos();
        let sin_pitch = self.pitch.sin();

        let forward = [
            cos_yaw * cos_pitch,
            sin_pitch,
            sin_yaw * cos_pitch,
        ];

        let right = [
            sin_yaw,
            0.0,
            -cos_yaw,
        ];

        let up = [
            -cos_yaw * sin_pitch,
            cos_pitch,
            -sin_yaw * sin_pitch,
        ];

        let eye = self.position;

        [
            [right[0], up[0], -forward[0], 0.0],
            [right[1], up[1], -forward[1], 0.0],
            [right[2], up[2], -forward[2], 0.0],
            [
                -(right[0] * eye[0] + right[1] * eye[1] + right[2] * eye[2]),
                -(up[0] * eye[0] + up[1] * eye[1] + up[2] * eye[2]),
                forward[0] * eye[0] + forward[1] * eye[1] + forward[2] * eye[2],
                1.0,
            ],
        ]
    }

    fn move_forward(&mut self, amount: f32) {
        self.position[0] += self.yaw.cos() * amount;
        self.position[2] += self.yaw.sin() * amount;
    }

    fn move_right(&mut self, amount: f32) {
        self.position[0] += self.yaw.sin() * amount;
        self.position[2] -= self.yaw.cos() * amount;
    }
}

// Генератор геометрии
struct GeometryBuilder {
    vertices: Vec<Vertex>,
}

impl GeometryBuilder {
    fn new() -> Self {
        Self { vertices: Vec::new() }
    }

    // Добавить куб
    fn add_cube(&mut self, center: [f32; 3], size: f32, color: [f32; 4]) {
        let half = size * 0.5;
        let x = center[0];
        let y = center[1];
        let z = center[2];

        // 6 граней, каждая по 2 треугольника (6 вершин)
        let faces = [
            // Передняя (z+)
            ([x-half, y-half, z+half], [x+half, y-half, z+half], [x+half, y+half, z+half], [x-half, y+half, z+half]),
            // Задняя (z-)
            ([x+half, y-half, z-half], [x-half, y-half, z-half], [x-half, y+half, z-half], [x+half, y+half, z-half]),
            // Левая (x-)
            ([x-half, y-half, z-half], [x-half, y-half, z+half], [x-half, y+half, z+half], [x-half, y+half, z-half]),
            // Правая (x+)
            ([x+half, y-half, z+half], [x+half, y-half, z-half], [x+half, y+half, z-half], [x+half, y+half, z+half]),
            // Верхняя (y+)
            ([x-half, y+half, z+half], [x+half, y+half, z+half], [x+half, y+half, z-half], [x-half, y+half, z-half]),
            // Нижняя (y-)
            ([x-half, y-half, z-half], [x+half, y-half, z-half], [x+half, y-half, z+half], [x-half, y-half, z+half]),
        ];

        for (v0, v1, v2, v3) in faces {
            // Треугольник 1
            self.vertices.push(Vertex { position: v0, color });
            self.vertices.push(Vertex { position: v1, color });
            self.vertices.push(Vertex { position: v2, color });
            // Треугольник 2
            self.vertices.push(Vertex { position: v0, color });
            self.vertices.push(Vertex { position: v2, color });
            self.vertices.push(Vertex { position: v3, color });
        }
    }

    // Добавить пол (плоскость)
    fn add_floor(&mut self, y: f32, size: f32, color: [f32; 4]) {
        let half = size * 0.5;
        self.vertices.push(Vertex { position: [-half, y, -half], color });
        self.vertices.push(Vertex { position: [half, y, -half], color });
        self.vertices.push(Vertex { position: [half, y, half], color });

        self.vertices.push(Vertex { position: [-half, y, -half], color });
        self.vertices.push(Vertex { position: [half, y, half], color });
        self.vertices.push(Vertex { position: [-half, y, half], color });
    }

    // Добавить сетку на полу
    fn add_grid(&mut self, y: f32, size: f32, divisions: i32, color: [f32; 4]) {
        let half = size * 0.5;
        let step = size / divisions as f32;

        for i in 0..=divisions {
            let pos = -half + i as f32 * step;

            // Линии по X
            self.vertices.push(Vertex { position: [pos, y, -half], color });
            self.vertices.push(Vertex { position: [pos, y, half], color });

            // Линии по Z
            self.vertices.push(Vertex { position: [-half, y, pos], color });
            self.vertices.push(Vertex { position: [half, y, pos], color });
        }
    }

    fn build(self) -> Vec<Vertex> {
        self.vertices
    }
}

fn main() {
    println!("🎮 AlKAsH3D - Walking World");
    println!("================================");
    println!("WASD - движение");
    println!("Мышь - осмотр");
    println!("ESC - выход");
    println!("================================");

    let event_loop = EventLoop::new();
    let window = WindowBuilder::new()
        .with_title("AlKAsH3D - Walking World")
        .with_inner_size(PhysicalSize::new(1280, 720))
        .build(&event_loop)
        .unwrap();

    // Захватываем курсор
    window.set_cursor_grab(true).unwrap();
    window.set_cursor_visible(false);

    let hwnd = window.hwnd();
    let size = window.inner_size();

    unsafe {
        // Инициализация D3D12
        let device = create_device();
        let queue = create_command_queue(device);
        let swap = create_swap_chain(queue, hwnd as usize, size.width, size.height);

        create_command_allocators(device, 3);
        let cmd_list = create_command_list(device);
        create_fence(device);

        let root_sig = create_root_signature(device);
        let pso = create_simple_pso(device, root_sig);

        let rtv_heap = create_descriptor_heap(device, 2, 0, false);
        let dsv_heap = create_descriptor_heap(device, 1, 1, false);

        let depth_buffer = create_depth_buffer(device, size.width, size.height);
        let dsv_handle = GetCPUDescriptorHandleForHeapStart(dsv_heap);
        create_depth_stencil_view(device, depth_buffer, dsv_handle);

        // Строим мир
        let mut builder = GeometryBuilder::new();

        // Пол
        builder.add_floor(0.0, 20.0, [0.3, 0.3, 0.4, 1.0]);

        // Сетка
        builder.add_grid(0.01, 20.0, 20, [0.5, 0.5, 0.6, 1.0]);

        // Красный куб
        builder.add_cube([-3.0, 1.0, 0.0], 2.0, [1.0, 0.2, 0.2, 1.0]);

        // Зелёный куб
        builder.add_cube([3.0, 1.0, 2.0], 2.0, [0.2, 1.0, 0.2, 1.0]);

        // Синий куб
        builder.add_cube([0.0, 1.0, -4.0], 2.0, [0.2, 0.2, 1.0, 1.0]);

        // Жёлтая колонна
        builder.add_cube([-2.0, 2.0, 4.0], 1.0, [1.0, 1.0, 0.2, 1.0]);
        builder.add_cube([-2.0, 4.0, 4.0], 1.0, [1.0, 0.8, 0.2, 1.0]);

        // Фиолетовая башня
        builder.add_cube([4.0, 1.5, -3.0], 3.0, [0.8, 0.2, 1.0, 1.0]);

        let vertices = builder.build();
        println!("Мир построен: {} вершин", vertices.len());

        // Вершинный буфер
        let vb = create_buffer(device, std::mem::size_of_val(&vertices[..]), ptr::null());
        update_subresource(vb, vertices.as_ptr() as *const c_void, std::mem::size_of_val(&vertices[..]));
        let vb_gpu_addr = get_buffer_gpu_address(vb);

        // Константный буфер
        let cb = create_buffer(device, std::mem::size_of::<MVP>(), ptr::null());
        let cb_gpu_addr = get_buffer_gpu_address(cb);

        // Камера и ввод
        let mut camera = Camera::new();
        let mut mouse_sensitivity = 0.002;
        let mut move_speed = 0.1;
        let mut keys = [false; 256];

        let mut last_time = Instant::now();

        event_loop.run(move |event, _, control_flow| {
            *control_flow = ControlFlow::Poll;

            match event {
                Event::WindowEvent { event, .. } => match event {
                    WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
                    WindowEvent::KeyboardInput { input: KeyboardInput { virtual_keycode: Some(key), state, .. }, .. } => {
                        let pressed = state == ElementState::Pressed;
                        match key {
                            VirtualKeyCode::Escape => *control_flow = ControlFlow::Exit,
                            VirtualKeyCode::W => keys[b'W' as usize] = pressed,
                            VirtualKeyCode::S => keys[b'S' as usize] = pressed,
                            VirtualKeyCode::A => keys[b'A' as usize] = pressed,
                            VirtualKeyCode::D => keys[b'D' as usize] = pressed,
                            VirtualKeyCode::LShift => move_speed = if pressed { 0.2 } else { 0.1 },
                            _ => {}
                        }
                    }
                    _ => {}
                },
                Event::DeviceEvent { event: DeviceEvent::MouseMotion { delta }, .. } => {
                    camera.yaw += delta.0 as f32 * mouse_sensitivity;
                    camera.pitch -= delta.1 as f32 * mouse_sensitivity;
                    camera.pitch = camera.pitch.clamp(-1.5, 1.5);
                }
                Event::MainEventsCleared => {
                    // Обновление движения
                    let now = Instant::now();
                    let dt = (now - last_time).as_secs_f32();
                    last_time = now;

                    if keys[b'W' as usize] {
                        camera.move_forward(move_speed * dt * 60.0);
                    }
                    if keys[b'S' as usize] {
                        camera.move_forward(-move_speed * dt * 60.0);
                    }
                    if keys[b'A' as usize] {
                        camera.move_right(-move_speed * dt * 60.0);
                    }
                    if keys[b'D' as usize] {
                        camera.move_right(move_speed * dt * 60.0);
                    }

                    window.request_redraw();
                }
                Event::RedrawRequested(_) => {
                    let size = window.inner_size();

                    // Рендеринг
                    begin_frame();

                    let frame_index = get_frame_index();
                    let rtv_handle = create_rtv_for_swapchain_buffer(device, swap, rtv_heap, frame_index);

                    let clear_color = [0.05f32, 0.05, 0.1, 1.0];
                    clear_render_target(rtv_handle, clear_color.as_ptr());
                    clear_depth_stencil(dsv_handle, 1.0, 0);

                    set_render_targets_with_depth(rtv_handle, dsv_handle, 1);
                    set_viewport(0.0, 0.0, size.width as f32, size.height as f32, 0.0, 1.0);
                    set_scissor_rect(0, 0, size.width as i32, size.height as i32);

                    set_graphics_pipeline(pso);
                    set_root_signature(root_sig);

                    // Матрицы
                    let proj = MVP::perspective(70.0, size.width as f32 / size.height as f32, 0.1, 100.0);
                    let view = camera.get_view_matrix();

                    let mut mvp = MVP::identity();
                    for i in 0..4 {
                        for j in 0..4 {
                            mvp.mvp[i][j] = 0.0;
                            for k in 0..4 {
                                mvp.mvp[i][j] += proj.mvp[i][k] * view[k][j];
                            }
                        }
                    }

                    update_subresource(cb, &mvp as *const MVP as *const c_void, std::mem::size_of::<MVP>());
                    set_root_constant_buffer_view(0, cb_gpu_addr);

                    set_vertex_buffer(vb_gpu_addr, (vertices.len() * std::mem::size_of::<Vertex>()) as u32, std::mem::size_of::<Vertex>() as u32);
                    set_primitive_topology(4);

                    draw_instanced(vertices.len() as u32, 1, 0, 0);

                    end_frame();
                    present_swap_chain(swap, 1);
                    wait_for_gpu();
                }
                _ => {}
            }
        });
    }
}