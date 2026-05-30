// tests/test_engine.rs
//! Интеграционные тесты для Alkash3D Engine

#[cfg(test)]
mod tests {
    use alkash3d_rs::*;
    use std::time::Instant;

    // ===================================================================
    // ТЕСТ 1: Создание устройства и базовых объектов
    // ===================================================================
    #[test]
    fn test_device_creation() {
        println!("\n=== TEST: Device Creation ===");

        let device = create_device();
        assert!(!device.is_null(), "Failed to create D3D12 device");

        let vram = get_gpu_vram_mb();
        println!("GPU VRAM: {} MB", vram);
        println!("Is real GPU: {}", is_real_gpu());

        let rtv_size = get_rtv_descriptor_size();
        let dsv_size = get_dsv_descriptor_size();
        let cbv_size = get_cbv_srv_uav_descriptor_size();

        println!("Descriptor sizes - RTV: {}, DSV: {}, CBV: {}", rtv_size, dsv_size, cbv_size);

        assert!(rtv_size > 0, "RTV descriptor size should be > 0");
        assert!(dsv_size > 0, "DSV descriptor size should be > 0");
        assert!(cbv_size > 0, "CBV descriptor size should be > 0");

        force_cleanup();
        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 2: Создание буферов
    // ===================================================================
    #[test]
    fn test_buffer_creation() {
        println!("\n=== TEST: Buffer Creation ===");

        let device = create_device();
        assert!(!device.is_null());

        // Создаём буфер 1KB
        let buffer = create_buffer(device, 1024, 0);
        assert!(!buffer.is_null(), "Failed to create upload buffer");

        let size = get_buffer_size(buffer);
        assert_eq!(size, 1024, "Buffer size mismatch");
        println!("Upload buffer created: {} bytes", size);

        // Создаём default буфер
        let default_buffer = create_buffer(device, 2048, 1);
        assert!(!default_buffer.is_null(), "Failed to create default buffer");
        println!("Default buffer created: {} bytes", get_buffer_size(default_buffer));

        // Очистка
        release_resource(buffer);
        release_resource(default_buffer);
        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 3: Запись в буфер и чтение
    // ===================================================================
    #[test]
    fn test_buffer_read_write() {
        println!("\n=== TEST: Buffer Read/Write ===");

        let device = create_device();
        assert!(!device.is_null());

        // Создаём upload буфер
        let buffer = create_buffer(device, 256, 0);
        assert!(!buffer.is_null());

        // Записываем тестовые данные
        let test_data: [u8; 16] = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15];
        let result = update_subresource(buffer, test_data.as_ptr() as *const std::ffi::c_void, 16);
        assert!(result, "Failed to write to buffer");
        println!("Wrote 16 bytes to buffer");

        // Читаем обратно
        let mapped = map_buffer(buffer);
        assert!(!mapped.is_null(), "Failed to map buffer");

        unsafe {
            let data = std::slice::from_raw_parts(mapped as *const u8, 16);
            assert_eq!(&data, &test_data, "Data mismatch");
            println!("Read back data: {:?}", &data[..8]);
        }

        unmap_buffer(buffer, 0, 16);
        release_resource(buffer);
        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 4: Создание и компиляция шейдеров
    // ===================================================================
    #[test]
    fn test_shader_compilation() {
        println!("\n=== TEST: Shader Compilation ===");

        // Вершинный шейдер
        let vs_blob = get_builtin_vs_blob();
        assert!(!vs_blob.is_null(), "Failed to compile vertex shader");

        let vs_size = get_blob_size(vs_blob);
        assert!(vs_size > 0, "Vertex shader size is 0");
        println!("Vertex shader compiled: {} bytes", vs_size);

        // Пиксельный шейдер
        let ps_blob = get_builtin_ps_blob();
        assert!(!ps_blob.is_null(), "Failed to compile pixel shader");

        let ps_size = get_blob_size(ps_blob);
        assert!(ps_size > 0, "Pixel shader size is 0");
        println!("Pixel shader compiled: {} bytes", ps_size);

        // Расширенные шейдеры
        let adv_vs = get_advanced_vs_blob();
        assert!(!adv_vs.is_null(), "Failed to compile advanced VS");
        println!("Advanced VS: {} bytes", get_blob_size(adv_vs));

        let adv_ps = get_advanced_ps_blob();
        assert!(!adv_ps.is_null(), "Failed to compile advanced PS");
        println!("Advanced PS: {} bytes", get_blob_size(adv_ps));

        free_blob(vs_blob);
        free_blob(ps_blob);
        free_blob(adv_vs);
        free_blob(adv_ps);
        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 5: Создание PSO
    // ===================================================================
    #[test]
    fn test_pso_creation() {
        println!("\n=== TEST: PSO Creation ===");

        let device = create_device();
        assert!(!device.is_null());

        let root_sig = create_root_signature(device);
        assert!(!root_sig.is_null(), "Failed to create root signature");
        println!("Root signature created");

        let pso = create_pso(device, root_sig, 0);
        assert!(!pso.is_null(), "Failed to create PSO");
        println!("PSO created");

        destroy_pso(pso);
        destroy_root_signature(root_sig);
        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 6: Форматы файлов (.altex)
    // ===================================================================
    #[test]
    fn test_altex_format() {
        println!("\n=== TEST: Altex Format ===");

        let mut altex = AltexFile::new();

        // Добавляем строку
        let name_id = altex.add_string("TestMesh");
        assert_eq!(name_id, 0);

        // Создаём простой куб (8 вершин, 12 индексов)
        let vertices = vec![
            Vertex { position: [-1.0, -1.0, -1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [ 1.0, -1.0, -1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [ 1.0, -1.0,  1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [-1.0, -1.0,  1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [-1.0,  1.0, -1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [0.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [ 1.0,  1.0, -1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [1.0, 0.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [ 1.0,  1.0,  1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [1.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
            Vertex { position: [-1.0,  1.0,  1.0], normal: [0.0, 0.0, 0.0], tangent: [0.0, 0.0, 0.0], bitangent: [0.0, 0.0, 0.0], uv: [0.0, 1.0], uv2: [0.0, 0.0], color: [1.0, 1.0, 1.0, 1.0] },
        ];

        let indices = vec![
            0,1,2, 0,2,3,  // bottom
            4,6,5, 4,7,6,  // top
            0,4,1, 1,4,5,  // front
            2,6,3, 3,6,7,  // back
            0,3,4, 3,7,4,  // left
            1,5,2, 2,5,6,  // right
        ];

        let mesh_id = altex.add_mesh(vertices, indices, "Cube");
        println!("Mesh added with ID: {}", mesh_id);

        // Сохраняем
        let filename = "test_cube.altex";
        let result = altex.save(filename);
        assert!(result.is_ok(), "Failed to save .altex file: {:?}", result.err());
        println!("Saved to: {}", filename);

        // Загружаем обратно
        let loaded = AltexFile::load(filename);
        assert!(loaded.is_ok(), "Failed to load .altex file");
        let loaded = loaded.unwrap();

        assert_eq!(loaded.meshes.len(), 1, "Wrong number of meshes");
        assert_eq!(loaded.vertices.len(), 8, "Wrong number of vertices");
        assert_eq!(loaded.indices.len(), 36, "Wrong number of indices");
        println!("Loaded: {} meshes, {} vertices, {} indices",
                 loaded.meshes.len(), loaded.vertices.len(), loaded.indices.len());

        // Удаляем тестовый файл
        let _ = std::fs::remove_file(filename);
        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 7: Формат автомобилей (.alcar)
    // ===================================================================
    #[test]
    fn test_alcar_format() {
        println!("\n=== TEST: Alcar Format ===");

        let sport_car = AlcarFile::create_sports_car();
        println!("Sports car: {} HP, top speed {} km/h",
                 sport_car.physics.engine_power, sport_car.physics.top_speed);

        let police_car = AlcarFile::create_police_car();
        println!("Police car: has siren: {}, AI script: {}",
                 police_car.lights.has_siren,
                 police_car.get_string(police_car.metadata.ai_script_id));

        // Сохраняем и загружаем
        let filename = "test_sports_car.alcar";
        let result = sport_car.save(filename);
        assert!(result.is_ok());

        let loaded = AlcarFile::load(filename);
        assert!(loaded.is_ok());
        let loaded = loaded.unwrap();

        assert_eq!(loaded.physics.top_speed, 320.0);
        assert_eq!(loaded.physics.gears, 7);
        println!("Loaded sports car: {} HP, top speed {} km/h",
                 loaded.physics.engine_power, loaded.physics.top_speed);

        let _ = std::fs::remove_file(filename);
        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 8: Планировщик задач
    // ===================================================================
    #[test]
    fn test_scheduler() {
        println!("\n=== TEST: Scheduler ===");

        let scheduler = EngineScheduler::new();
        println!("CPU cores: {}", num_cpus::get());
        println!("Broad phase threshold: {}", scheduler.broad_phase_threshold());
        println!("Narrow phase threshold: {}", scheduler.narrow_phase_threshold());

        let mut counter = 0;
        let counter_ref = &mut counter;

        // Выполняем задачу
        let result = scheduler.execute(
            Task::new(1, TaskPriority::High),
            move || {
                println!("Task executed on thread");
            }
        );
        assert!(result, "Failed to execute task");

        // Небольшая задержка для завершения задачи
        std::thread::sleep(std::time::Duration::from_millis(10));

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 9: Аллокаторы команд (D3D12)
    // ===================================================================
    #[test]
    fn test_command_allocators() {
        println!("\n=== TEST: Command Allocators ===");

        let device = create_device();
        assert!(!device.is_null());

        let result = create_command_allocators(device, 3);
        assert!(result, "Failed to create command allocators");

        let fence_result = create_fence(device);
        assert!(fence_result, "Failed to create fence");

        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 10: Производительность (бенчмарк)
    // ===================================================================
    #[test]
    fn test_performance() {
        println!("\n=== TEST: Performance Benchmark ===");

        let device = create_device();
        assert!(!device.is_null());

        let scheduler = EngineScheduler::new();

        // Создаём движок
        let mut engine = AlkashEngine::new();

        // Добавляем тестовые объекты
        for i in 0..100 {
            let x = (i as f32 - 50.0) * 2.0;
            let z = (i as f32 - 50.0) * 2.0;
            engine.add_sphere_body(x, 10.0, z, 1.0);
        }

        // Замеряем производительность
        let iterations = 100;
        let start = Instant::now();

        for i in 0..iterations {
            let camera = [0.0, 10.0, 20.0];
            let view_proj = [1.0, 0.0, 0.0, 0.0,
                0.0, 1.0, 0.0, 0.0,
                0.0, 0.0, 1.0, 0.0,
                0.0, 0.0, -10.0, 1.0];

            engine.update(0.016, -9.81, camera, view_proj);

            if i % 20 == 0 {
                println!("Frame {} complete", i);
            }
        }

        let elapsed = start.elapsed();
        let avg_ms = elapsed.as_secs_f32() * 1000.0 / iterations as f32;

        println!("{} iterations in {:.2} ms", iterations, elapsed.as_secs_f32() * 1000.0);
        println!("Average frame time: {:.2} ms", avg_ms);
        println!("FPS: {:.1}", 1000.0 / avg_ms);

        force_cleanup();

        assert!(avg_ms < 100.0, "Performance too slow: {:.2} ms per frame", avg_ms);
        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 11: Формат мира (.alworld)
    // ===================================================================
    #[test]
    fn test_alworld_format() {
        println!("\n=== TEST: Alworld Format ===");

        let world = AlworldFile::new(4.0);
        println!("World bounds: {:?} to {:?}", world.header.world_bounds_min, world.header.world_bounds_max);
        println!("Total chunks: {}", world.header.total_chunks);
        println!("Load distance: {} m", world.streaming_config.load_distance);

        let demo = AlworldFile::create_open_world_demo();
        println!("Demo world chunks: {}", demo.chunks.len());

        force_cleanup();

        println!("✅ Test passed!");
    }

    // ===================================================================
    // ТЕСТ 12: Формат киносцен (.aluv)
    // ===================================================================
    #[test]
    fn test_aluv_format() {
        println!("\n=== TEST: Aluv Format ===");

        let cinematic = AluvFile::create_opening_cinematic();
        println!("Cinematic sequences: {}", cinematic.header.sequence_count);
        println!("Total duration: {} ms", cinematic.header.total_duration_ms);

        assert_eq!(cinematic.sequences.len(), 1);
        assert_eq!(cinematic.subtitles.len(), 1);

        let seq = &cinematic.sequences[0];
        println!("Sequence: {} ms, {} fps", seq.duration_ms, seq.fps);

        let sub = &cinematic.subtitles[0];
        println!("Subtitle: {} - {} ms",
                 cinematic.get_string(sub.text_id), sub.time_start_ms);

        force_cleanup();

        println!("✅ Test passed!");
    }
}