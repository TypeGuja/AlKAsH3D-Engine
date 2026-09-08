//! Загрузка геометрии и текстур из `.altex` в GPU-ресурсы: конвертация
//! вершин, сборка `Mesh` из распарсенного файла (синхронно или из уже
//! прочитанных фоновым потоком данных), материальный SRV-хип (кэш текстур,
//! ленивые нейтральные fallback-текстуры: белая/плоская normal/MR).
//!
//! ВЫНЕСЕНО из `engine/mod.rs` (Фаза 1 архитектурного рефакторинга — разбивка
//! монолита `impl AlkashEngine` на подсистемы). Перенос дословный, тела
//! методов не менялись — видимость некоторых методов поднята до
//! `pub(super)`, потому что их вызывают `load_chunk_sync_fallback`/
//! `integrate_loaded_chunk` (world streaming, отдельный подмодуль) и
//! `render_frame` (остаются в mod.rs/render_frame.rs).

use std::sync::Arc;
use windows::core::*;
use windows::Win32::Foundation::*;
use super::{AlkashEngine, Vertex, Mesh, NUM_CASCADES};
use crate::STATE;

impl AlkashEngine {
    /// ДОБАВЛЕНО (Задача #14: загрузчик .altex -> GPU Mesh). Возвращает
    /// список mesh_index (по одному на каждый `altex_format::Mesh` внутри
    /// файла) для объекта чанка, ссылающегося на geometry-файл по пути
    /// `altex_path`.
    ///
    /// Поведение:
    /// - `altex_path == "placeholder"` (используется текущим демо-генератором
    ///   мира, см. `AlworldFile::create_and_save_demo_world`) — сразу
    ///   fallback на единичный куб, реального файла не существует и не
    ///   должно быть попытки его открыть.
    /// - Путь уже встречался и успешно распарсен раньше — отдаём
    ///   закэшированный `Vec<usize>` из `self.altex_mesh_cache` без
    ///   повторного чтения диска/пересоздания GPU-ресурсов (один и тот же
    ///   файл обычно используется тысячами объектов открытого мира —
    ///   фонарные столбы, деревья и т.д.).
    /// - Файл не найден / повреждён / не парсится — WARNING в лог и
    ///   fallback на placeholder-куб (та же логика отказоустойчивости, что
    ///   раньше была всегда включена, см. комментарий у `load_chunk`) —
    ///   один плохой ассет не должен останавливать стриминг всего мира.
    /// - Успешный парсинг — конвертируем каждый `altex_format::Vertex` в
    ///   `engine::Vertex` (см. `altex_vertex_to_engine_vertex`), строим по
    ///   одному `engine::Mesh` на каждый `altex_format::Mesh` через
    ///   `Mesh::from_vertices_and_indices` (индексы уже глобальные для
    ///   всего файла в `altex_format::Vertex`/`indices`, поэтому берём
    ///   срез vertices per-mesh и переиндексируем index_offset/count
    ///   относительно НАЧАЛА этого среза — см. ниже), регистрируем через
    ///   `self.add_mesh`, кэшируем и возвращаем результат.
    ///
    /// ПЕРЕИМЕНОВАНО (фоновая загрузка чанков): это ПОЛНОСТЬЮ синхронный
    /// путь (сам вызывает `AltexFile::load`, то есть сам читает файл с
    /// диска) — раньше был единственным способом (`load_object_mesh`),
    /// теперь используется только из `load_chunk_sync_fallback` (см. её
    /// комментарий). Штатный путь — `load_object_mesh_from_parsed`, для
    /// которого файл уже прочитан заранее фоновым потоком. Обе функции
    /// делят общую GPU-часть через `build_meshes_from_altex`, чтобы не
    /// дублировать логику работы с материалами/текстурами.
    pub(super) fn load_object_mesh_sync(&mut self, altex_path: &str) -> Vec<usize> {
        if altex_path.is_empty() || altex_path == "placeholder" {
            return vec![self.load_placeholder_mesh()];
        }

        if let Some(cached) = self.altex_mesh_cache.get(altex_path) {
            return cached.clone();
        }

        let altex = match crate::altex_format::AltexFile::load(altex_path) {
            Ok(file) => file,
            Err(e) => {
                eprintln!(
                    "[ENGINE] WARNING: не удалось загрузить .altex '{}': {:?} — используется placeholder-куб",
                    altex_path, e
                );
                let fallback = vec![self.load_placeholder_mesh()];
                self.altex_mesh_cache.insert(altex_path.to_string(), fallback.clone());
                return fallback;
            }
        };

        self.build_meshes_from_altex(&altex, altex_path)
    }

    /// ДОБАВЛЕНО (фоновая загрузка чанков): штатный путь загрузки геометрии
    /// объекта чанка — файл `.altex` УЖЕ прочитан и распарсен фоновой
    /// задачей пула планировщика (см. `world_streaming.rs::load_chunk_data`/
    /// `request_chunk_load`) и передан сюда готовым в `parsed_altex` (см.
    /// `ChunkLoadResult`). Эта функция
    /// НЕ трогает диск вообще, только GPU-ресурсы — тот же кэш
    /// `self.altex_mesh_cache` и та же логика fallback на placeholder-куб,
    /// что и в `load_object_mesh_sync`, см. её комментарий.
    pub(super) fn load_object_mesh_from_parsed(
        &mut self,
        altex_path: &str,
        parsed_altex: &std::collections::HashMap<String, std::result::Result<Arc<crate::altex_format::AltexFile>, String>>,
    ) -> Vec<usize> {
        if altex_path.is_empty() || altex_path == "placeholder" {
            return vec![self.load_placeholder_mesh()];
        }

        if let Some(cached) = self.altex_mesh_cache.get(altex_path) {
            return cached.clone();
        }

        let altex = match parsed_altex.get(altex_path) {
            Some(Ok(file)) => Arc::clone(file),
            Some(Err(e)) => {
                eprintln!(
                    "[ENGINE] WARNING: не удалось загрузить .altex '{}': {} — используется placeholder-куб",
                    altex_path, e
                );
                let fallback = vec![self.load_placeholder_mesh()];
                self.altex_mesh_cache.insert(altex_path.to_string(), fallback.clone());
                return fallback;
            }
            None => {
                eprintln!(
                    "[ENGINE] WARNING: .altex '{}' отсутствует в предзагруженных фоновым потоком данных — используется placeholder-куб",
                    altex_path
                );
                let fallback = vec![self.load_placeholder_mesh()];
                self.altex_mesh_cache.insert(altex_path.to_string(), fallback.clone());
                return fallback;
            }
        };

        self.build_meshes_from_altex(&altex, altex_path)
    }

    /// ДОБАВЛЕНО (фоновая загрузка чанков): общая GPU-часть загрузки
    /// `.altex` — раньше была "хвостом" единственной функции
    /// `load_object_mesh` ПОСЛЕ успешного чтения файла; вынесена сюда,
    /// чтобы `load_object_mesh_sync` (fallback) и `load_object_mesh_from_parsed`
    /// (штатный путь) не дублировали ~80 строк работы с материалами/
    /// текстурами. Тело — ДОСЛОВНО то же самое, что было в старой
    /// `load_object_mesh` после строки `let altex = ...`. Всегда
    /// вызывается с главного потока (создаёт GPU mesh/texture ресурсы).
    fn build_meshes_from_altex(&mut self, altex: &crate::altex_format::AltexFile, altex_path: &str) -> Vec<usize> {
        let mut mesh_indices = Vec::with_capacity(altex.meshes.len());
        for altex_mesh in &altex.meshes {
            let v_start = altex_mesh.vertex_offset as usize;
            let v_end = v_start + altex_mesh.vertex_count as usize;
            let i_start = altex_mesh.index_offset as usize;
            let i_end = i_start + altex_mesh.index_count as usize;

            if v_end > altex.vertices.len() || i_end > altex.indices.len() {
                eprintln!(
                    "[ENGINE] WARNING: .altex '{}' содержит меш с некорректными offset/count (вне границ vertices/indices) — меш пропущен",
                    altex_path
                );
                continue;
            }

            let engine_vertices: Vec<Vertex> = altex.vertices[v_start..v_end]
                .iter()
                .map(Self::altex_vertex_to_engine_vertex)
                .collect();

            let engine_indices: Vec<u32> = altex.indices[i_start..i_end]
                .iter()
                .map(|idx| idx - altex_mesh.vertex_offset)
                .collect();

            match Mesh::from_vertices_and_indices(&engine_vertices, &engine_indices) {
                Ok(mut mesh) => {
                    if let Some(material) = altex.materials.get(altex_mesh.material_id as usize) {
                        if material.albedo_map != 0xFFFFFFFF {
                            mesh.albedo_srv_index = self.load_altex_map_srv(altex, altex_path, material.albedo_map, "albedo");
                        }

                        if material.normal_map != 0xFFFFFFFF {
                            mesh.normal_srv_index = self.load_altex_map_srv(altex, altex_path, material.normal_map, "normal");
                        }

                        if material.metallic_map != 0xFFFFFFFF && material.metallic_map == material.roughness_map {
                            mesh.mr_srv_index = self.load_altex_map_srv(altex, altex_path, material.metallic_map, "metallic-roughness");
                        } else if material.metallic_map != 0xFFFFFFFF || material.roughness_map != 0xFFFFFFFF {
                            eprintln!(
                                "[ENGINE] WARNING: .altex '{}' содержит РАЗДЕЛЬНЫЕ metallic_map/roughness_map (индексы {} и {}) — объединение раздельных текстур в одну ORM-карту пока не реализовано, используются скалярные metallic={}/roughness={} материала",
                                altex_path, material.metallic_map, material.roughness_map, material.metallic, material.roughness
                            );
                        }

                        mesh.material_metallic = material.metallic;
                        mesh.material_roughness = material.roughness;
                    }

                    let index = self.add_mesh(mesh);
                    mesh_indices.push(index);
                }
                Err(e) => {
                    eprintln!(
                        "[ENGINE] WARNING: не удалось создать GPU Mesh из .altex '{}': {:?} — меш пропущен",
                        altex_path, e
                    );
                }
            }
        }

        if mesh_indices.is_empty() {
            mesh_indices.push(self.load_placeholder_mesh());
        }

        self.altex_mesh_cache.insert(altex_path.to_string(), mesh_indices.clone());
        mesh_indices
    }

    /// Конвертирует вершину формата `.altex` (position/normal/tangent/
    /// bitangent/uv/uv2/color — 7 полей) в вершину движка (position/normal/
    /// color/uv/tangent). ОБНОВЛЕНО (Задача #15, normal mapping): `tangent`
    /// теперь тоже переносится (был отброшен на предыдущем шаге этой же
    /// задачи, см. историю в git/предыдущих правках) — конвертируется из
    /// `.altex`-представления (tangent xyz + отдельный bitangent xyz) в
    /// компактный движковый формат (tangent xyz + w=handedness), см.
    /// `compute_tangent_handedness`. `uv2` (вторая UV-развёртка, обычно под
    /// lightmap/AO-запечёнку) по-прежнему отбрасывается — движок пока не
    /// поддерживает lightmap-запекание, это отдельное, не относящееся к
    /// normal mapping расширение.
    fn altex_vertex_to_engine_vertex(v: &crate::altex_format::Vertex) -> Vertex {
        let handedness = Self::compute_tangent_handedness(v.normal, v.tangent, v.bitangent);
        Vertex {
            position: [v.position[0], v.position[1], v.position[2], 1.0],
            normal: v.normal,
            color: v.color,
            uv: v.uv,
            tangent: [v.tangent[0], v.tangent[1], v.tangent[2], handedness],
        }
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping): вычисляет знак ("рукость")
    /// касательного базиса — ±1.0 — из явного normal/tangent/bitangent
    /// `.altex`-файла: `sign(dot(cross(normal, tangent), bitangent))`.
    /// Нужен, потому что движок хранит bitangent НЕ явным полем, а
    /// восстанавливает его в шейдере как `cross(normal, tangent.xyz) *
    /// tangent.w` (см. `Vertex::tangent` в engine/mod.rs) — этот знак
    /// компенсирует случаи, когда UV-развёртка отражена (мировая ось UV
    /// отзеркалена относительно объекта, частый случай для симметричной
    /// геометрии типа персонажей) и честный запечённый bitangent НЕ
    /// совпадает по направлению с `cross(normal, tangent)` "как есть".
    fn compute_tangent_handedness(normal: [f32; 3], tangent: [f32; 3], bitangent: [f32; 3]) -> f32 {
        let cross = [
            normal[1] * tangent[2] - normal[2] * tangent[1],
            normal[2] * tangent[0] - normal[0] * tangent[2],
            normal[0] * tangent[1] - normal[1] * tangent[0],
        ];
        let dot = cross[0] * bitangent[0] + cross[1] * bitangent[1] + cross[2] * bitangent[2];
        if dot < 0.0 { -1.0 } else { 1.0 }
    }

    /// Возвращает mesh_index единичного placeholder-куба, используемого,
    /// когда реальный `.altex` объекта недоступен (см. `load_object_mesh`).
    /// Переиспользует ОДИН созданный меш-куб для ВСЕХ таких случаев
    /// (кэшируется в `self.world_chunk_placeholder_mesh`) вместо создания
    /// нового меша на каждый объект — тысячи объектов открытого мира не
    /// должны означать тысячи идентичных GPU-мешей одного и того же куба.
    fn load_placeholder_mesh(&mut self) -> usize {
        if let Some(index) = self.world_chunk_placeholder_mesh {
            return index;
        }
        let index = self.add_cube(1.0);
        self.world_chunk_placeholder_mesh = Some(index);
        index
    }

    /// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Гарантирует, что
    /// `shadow_srv_heap` вмещает как минимум `NUM_CASCADES + needed`
    /// SRV-слотов — растёт степенями двойки (в material-части), как и
    /// `light_buffer_capacity`. Material-текстуры ОБЯЗАНЫ жить в ТОМ ЖЕ
    /// хипе, что и shadow-каскады (см. подробное объяснение у полей
    /// `material_srv_capacity`/`material_textures` — аппаратное
    /// ограничение D3D12: не более одного shader-visible CBV_SRV_UAV хипа
    /// забинжено одновременно). Дескрипторный хип НЕЛЬЗЯ просто
    /// пересоздать "пустым": каждый раз, когда хип пересоздаётся (новый
    /// COM-объект), ВСЕ ранее записанные в него SRV нужно записать ЗАНОВО
    /// в новый хип — старые GPU-адреса автоматически становятся
    /// недействительными вместе со старым хипом (тот же класс проблемы,
    /// что уже был решён для `renderer.srv_uav_heap` при resize окна, см.
    /// подробный комментарий в `on_resize`). Поэтому здесь ЗАНОВО
    /// создаются SRV И для NUM_CASCADES shadow-каскадов (из
    /// `self.shadow_maps`), И для ВСЕХ уже загруженных
    /// `self.material_textures` — не только для новых.
    ///
    /// ВАЖНО: вызывается ТОЛЬКО когда `shadow_srv_heap` уже существует
    /// (создаётся в `create_shadow_resources`, вызывается раньше первой
    /// загрузки любой текстуры в нормальном порядке инициализации
    /// движка) — если его почему-то ещё нет, рост material-части
    /// откладывается безопасно (see match ниже), а не паникует.
    fn ensure_material_srv_capacity(&mut self, needed: u32) -> Result<()> {
        if self.shadow_srv_heap.is_some() && needed <= self.material_srv_capacity {
            return Ok(());
        }

        let Some(_) = &self.shadow_srv_heap else {
            eprintln!("[ENGINE] WARNING: ensure_material_srv_capacity вызван до create_shadow_resources — текстура не зарегистрирована");
            return Err(Error::from_hresult(HRESULT(1)));
        };

        let new_material_capacity = needed.max(16).next_power_of_two();
        let total_slots = NUM_CASCADES as u32 + new_material_capacity;
        let heap = crate::heap::DescriptorHeap::create_cbv_srv_uav_heap(total_slots)?;

        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };

        for cascade in 0..NUM_CASCADES {
            if let Some(shadow_map) = &self.shadow_maps[cascade] {
                let cpu_handle = crate::heap::DescriptorHeap::get_cpu_handle(&heap, cascade as u32, cbv_srv_uav_size);
                if let Err(e) = shadow_map.create_shadow_srv(cpu_handle) {
                    eprintln!(
                        "[ENGINE] WARNING: не удалось повторно создать SRV shadow-каскада {} при росте shadow_srv_heap: {:?}",
                        cascade, e
                    );
                }
            }
        }

        for (index, texture) in self.material_textures.iter().enumerate() {
            let slot = NUM_CASCADES as u32 + index as u32;
            let cpu_handle = crate::heap::DescriptorHeap::get_cpu_handle(&heap, slot, cbv_srv_uav_size);
            if let Err(e) = texture.create_srv(cpu_handle) {
                eprintln!(
                    "[ENGINE] WARNING: не удалось повторно создать SRV текстуры {} при росте shadow_srv_heap: {:?}",
                    index, e
                );
            }
        }

        let srv_gpu = crate::heap::DescriptorHeap::get_gpu_handle(&heap, 0, cbv_srv_uav_size);

        println!(
            "[ENGINE] shadow_srv_heap перевыделен под материалы: {} слотов всего ({} каскадов + {} материалов, {} текстур перерегистрировано)",
            total_slots, NUM_CASCADES, new_material_capacity, self.material_textures.len()
        );

        self.shadow_srv_heap = Some(heap);
        self.shadow_srv_gpu = srv_gpu;
        self.material_srv_capacity = new_material_capacity;
        Ok(())
    }

    /// ДОБАВЛЕНО (Задача #15). Регистрирует новую GPU-текстуру (RGBA8,
    /// `pixels.len()` обязан быть РОВНО `width*height*4` — см. проверку в
    /// `Texture::create_texture2d`) в `shadow_srv_heap` (material-часть,
    /// см. `ensure_material_srv_capacity`) и возвращает её SRV-индекс
    /// (УЖЕ со смещением на NUM_CASCADES — то есть готовый
    /// `OffsetInDescriptorsFromTableStart` для root-параметра 5, register
    /// t6, см. render_frame). НЕ проверяет кэш сама — вызывающий код
    /// (`load_or_get_texture_srv`) отвечает за дедупликацию по ключу
    /// кэша, эта функция всегда создаёт РОВНО одну новую GPU-текстуру.
    fn register_material_texture(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<u32> {
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_R8G8B8A8_UNORM;

        let texture = crate::texture::Texture::create_texture2d(width, height, DXGI_FORMAT_R8G8B8A8_UNORM, Some(pixels))?;

        let local_index = self.material_texture_count;
        self.ensure_material_srv_capacity(local_index + 1)?;

        let cbv_srv_uav_size = {
            let state = STATE.lock().unwrap();
            state.cbv_srv_uav_descriptor_size
        };
        let slot = NUM_CASCADES as u32 + local_index;
        let heap = self.shadow_srv_heap.as_ref().unwrap();
        let cpu_handle = crate::heap::DescriptorHeap::get_cpu_handle(heap, slot, cbv_srv_uav_size);
        texture.create_srv(cpu_handle)?;

        self.material_textures.push(texture);
        self.material_texture_count += 1;
        Ok(slot)
    }

    /// ДОБАВЛЕНО (Задача #15). Нейтральная белая 1x1 RGBA8(255,255,255,255)
    /// текстура — fallback SRV для мешей без собственной albedo-текстуры
    /// (см. `Mesh::albedo_srv_index` и биндинг в render_frame). Создаётся
    /// ЛЕНИВО (только при первом реальном обращении — не в `Self::new()`),
    /// т.к. большинство существующих сцен (main1.rs/main2.rs, демо-мир до
    /// появления настоящих .altex с материалами) вообще не используют
    /// текстуры и не должны платить даже за одну лишнюю GPU-текстуру.
    pub(super) fn ensure_white_texture(&mut self) -> Result<u32> {
        if let Some(index) = self.white_texture_srv_index {
            return Ok(index);
        }
        let index = self.register_material_texture(1, 1, &[255, 255, 255, 255])?;
        self.white_texture_srv_index = Some(index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping). См. поле
    /// `flat_normal_srv_index` — та же ленивая lazy-init схема, что и
    /// `ensure_white_texture`.
    pub(super) fn ensure_flat_normal_texture(&mut self) -> Result<u32> {
        if let Some(index) = self.flat_normal_srv_index {
            return Ok(index);
        }
        let index = self.register_material_texture(1, 1, &[128, 128, 255, 255])?;
        self.flat_normal_srv_index = Some(index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping). См. поле
    /// `neutral_mr_srv_index` — та же ленивая lazy-init схема. Значения
    /// пикселей (0,128,0,255) сами по себе не используются шейдером в
    /// fallback-случае (root constants приоритетнее — см. HLSL PS main()),
    /// выбраны просто как безобидные валидные байты на случай будущего
    /// расширения, где эта текстура станет реально читаемой.
    pub(super) fn ensure_neutral_mr_texture(&mut self) -> Result<u32> {
        if let Some(index) = self.neutral_mr_srv_index {
            return Ok(index);
        }
        let index = self.register_material_texture(1, 1, &[0, 128, 0, 255])?;
        self.neutral_mr_srv_index = Some(index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (My Summer Car-like демо — процедурные текстуры без PNG/
    /// внешних файлов, см. `src/proc_textures.rs`): публичная обёртка над
    /// `register_material_texture` — тот же самый путь регистрации GPU-
    /// текстуры, которым уже пользуется загрузчик `.altex`
    /// (`load_altex_map_srv`), но доступная из bin-файлов (сам
    /// `register_material_texture` — приватный `fn` модуля `engine`, bin-
    /// файлы позвать его не могут). В отличие от `load_or_get_texture_srv`
    /// — НЕ кэширует по пути файла (процедурным текстурам физического
    /// файла на диске не существует); вызывающий код (генератор в
    /// `proc_textures.rs`/`main_car.rs`) сам вызывает это РОВНО один раз на
    /// уникальную текстуру и переиспользует возвращённый индекс для всех
    /// мешей, которым она нужна — повторный вызов создал бы вторую
    /// идентичную GPU-текстуру впустую. `pixels.len()` обязан быть РОВНО
    /// `width*height*4` (RGBA8), как и у приватной версии.
    pub fn create_texture_rgba(&mut self, width: u32, height: u32, pixels: &[u8]) -> Option<u32> {
        match self.register_material_texture(width, height, pixels) {
            Ok(index) => Some(index),
            Err(e) => {
                eprintln!("[ENGINE] WARNING: create_texture_rgba({}x{}) failed: {:?}", width, height, e);
                None
            }
        }
    }

    /// ДОБАВЛЕНО (Задача #15). Загружает (или берёт из кэша) SRV-индекс
    /// albedo-текстуры конкретного материала конкретного .altex-файла.
    /// Ключ кэша — `"{altex_path}#{albedo_map_index}"`, а НЕ просто
    /// `altex_path`: один .altex файл может содержать НЕСКОЛЬКО разных
    /// текстур (см. `AltexFile::textures`), поэтому индекс текстуры
    /// внутри файла обязателен, иначе разные текстуры одного файла
    /// схлопнулись бы в один и тот же кэшированный SRV.
    fn load_or_get_texture_srv(&mut self, altex_path: &str, albedo_map_index: u32, width: u32, height: u32, pixels: &[u8]) -> Result<u32> {
        let cache_key = format!("{}#{}", altex_path, albedo_map_index);
        if let Some(&index) = self.texture_cache.get(&cache_key) {
            return Ok(index);
        }
        let index = self.register_material_texture(width, height, pixels)?;
        self.texture_cache.insert(cache_key, index);
        Ok(index)
    }

    /// ДОБАВЛЕНО (Задача #15, normal mapping): общий хелпер поверх
    /// `load_or_get_texture_srv`, вынесенный из `load_object_mesh` — та же
    /// логика (границы `texture_data`, кэш по `"{altex_path}#{map_index}"`,
    /// нефатальная ошибка → `eprintln!` + `None`) нужна ТРИЖДЫ на меш
    /// (albedo/normal/metallic-roughness), раньше существовала только
    /// заинлайненной под albedo. `altex` — отдельный параметр (не поле
    /// `self`), поэтому одновременное заимствование `&AltexFile` и
    /// `&mut self` (для `self.load_or_get_texture_srv` внутри) не
    /// конфликтует с borrow checker'ом.
    fn load_altex_map_srv(&mut self, altex: &crate::altex_format::AltexFile, altex_path: &str, map_index: u32, map_kind: &str) -> Option<u32> {
        let texture = altex.textures.get(map_index as usize)?;
        let tex_start = texture.data_offset as usize;
        let tex_end = tex_start + texture.data_size as usize;
        if tex_end > altex.texture_data.len() {
            eprintln!(
                "[ENGINE] WARNING: .altex '{}' содержит {}-текстуру с некорректными data_offset/data_size (вне границ texture_data) — меш без этой карты",
                altex_path, map_kind
            );
            return None;
        }
        let pixels = &altex.texture_data[tex_start..tex_end];
        match self.load_or_get_texture_srv(altex_path, map_index, texture.width, texture.height, pixels) {
            Ok(srv_index) => Some(srv_index),
            Err(e) => {
                eprintln!(
                    "[ENGINE] WARNING: не удалось создать SRV {}-текстуры для .altex '{}': {:?} — меш без этой карты",
                    map_kind, altex_path, e
                );
                None
            }
        }
    }
}
