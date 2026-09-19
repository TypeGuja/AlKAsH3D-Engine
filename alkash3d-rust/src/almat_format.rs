// almat_format.rs - Material Acceleration System

use std::io::{Read, Write};

#[repr(C)]
pub struct AlmatHeader {
    pub magic: [u8; 8],           // "ALKALMAT"
    pub version: u32,
    pub total_materials: u32,     // ДОБАВЛЕНО: теперь = material_definitions.len() после save() (см. ниже)
    pub material_buckets: u32,    // Группировка по типам (opaque, transparent, decal)
    pub string_table_offset: u64,
    pub bucket_table_offset: u64,
    pub material_table_offset: u64,
    pub texture_atlas_offset: u64,
    // ДОБАВЛЕНО (авторские материалы — по прямому запросу пользователя,
    // "доделываем .almat"): смещение таблицы `MaterialDefinition` —
    // см. подробное объяснение у самой структуры и у `AlmatFile::save`/
    // `load` ниже про то, почему это ОТДЕЛЬНАЯ секция, а не переиспользование
    // `AcceleratedMaterial`. Заменяет прежнее неиспользуемое поле
    // `shader_cache_offset` (в движке никогда не существовало формата
    // "shader cache" — ни одной структуры под него не было, а `save()`/
    // `load()` для `AlmatFile` вообще не существовало до этой правки, так
    // что переименовать поле, а не оставлять рядом с ним мёртвое имя,
    // ничего не ломает — совместимости на диске ещё ни у кого нет).
    pub material_definitions_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MaterialBucket {
    pub bucket_type: u32,         // 0=opaque, 1=alpha_test, 2=transparent, 3=decal
    pub material_start: u32,
    pub material_count: u32,
    pub sort_key: u32,            // Для сортировки при рендеринге
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AcceleratedMaterial {
    pub name_id: u32,
    pub shader_hash: u64,         // Хеш комбинации шейдеров для быстрого поиска
    pub texture_handles: [u64; 8], // Прямые GPU-дескрипторы текстур
    pub constant_buffer_data: [u32; 16], // Предзапечённые константы
    pub render_state_hash: u64,   // Хеш состояния рендера для батчинга
    pub batch_group: u32,         // Группа батчинга
    pub lod_material_id: u32,     // ID материала для LOD версий
    pub draw_calls_per_frame: u32, // Статистика использования
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct TextureAtlasEntry {
    pub texture_name_id: u32,
    pub atlas_x: u16,
    pub atlas_y: u16,
    pub atlas_width: u16,
    pub atlas_height: u16,
    pub page_index: u16,          // Для больших атласов из нескольких страниц
}

/// ДОБАВЛЕНО (авторские материалы — по прямому запросу пользователя):
/// `AcceleratedMaterial` выше — это НЕ формат для авторинга, а внутренняя
/// GPU-структура ускорения рендера (батчинг/сортировка) — `texture_handles`
/// это СЫРЫЕ GPU-дескрипторы, валидные только внутри одной запущенной
/// GPU-сессии (сохранить их на диск и прочитать в другом запуске
/// бессмысленно — они ничего не будут указывать), а `constant_buffer_data`
/// — уже ЗАПЕЧЁННЫЙ (baked) blob констант шейдера, из которого нельзя
/// обратно извлечь "какой был цвет/металличность" без знания точной
/// раскладки конкретного шейдера на момент запекания. Ни редактор, ни
/// человек не может осмысленно прочитать/написать такую структуру.
///
/// `MaterialDefinition` — параллельная, ЧЕЛОВЕКОЧИТАЕМАЯ секция с реальными
/// PBR-параметрами (albedo/metallic/roughness/emissive) — форма один в один
/// повторяет `altex_format::Material` (тот же набор полей, та же логика
/// текстурных слотов), НО ссылки на текстуры — это индексы в
/// `AlmatFile::strings` (пути к файлам), а не встроенные пиксели, как в
/// `.altex`: `.almat` задуман как ПЕРЕИСПОЛЬЗУЕМАЯ библиотека материалов
/// (один материал, используемый многими объектами/сценами), а не
/// самодостаточный пакет геометрии+текстур одного объекта — встраивать
/// текстуры заново в каждый `.almat` означало бы дублировать одни и те же
/// байты текстуры на каждый материал, который её использует.
///
/// Движок при загрузке уровня строит `AcceleratedMaterial` ИЗ
/// `MaterialDefinition` (компилирует шейдер/считает хеши/резолвит
/// GPU-дескрипторы текстур) — так `AcceleratedMaterial` остаётся чисто
/// рантайм-кэшем, не полем ввода.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct MaterialDefinition {
    pub name_id: u32,
    pub albedo: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub ao: f32,
    pub emissive: [f32; 3],
    // Индексы в `AlmatFile::strings` (пути к файлам текстур на диске),
    // `NO_TEXTURE` = слот не используется. ОБНОВЛЕНО (текстуры материалов —
    // по прямому запросу пользователя): эдитор теперь заполняет
    // `albedo_texture_id` при экспорте, если у материала назначена картинка
    // с известным путём на диске (`material::Material::albedo_texture`,
    // `TextureAsset::source_path` — см. `alkash3d-editorapp/src/converters/
    // almat.rs`); текстура без пути (например встроенная только байтами из
    // чужого `.altex`, без файла на диске эдитора) экспортируется как
    // `NO_TEXTURE` — сослаться из `.almat` попросту не на что. Остальные
    // четыре слота ниже (normal/metallic_roughness/ao/emissive) эдитор всё
    // ещё не поддерживает и пишет `NO_TEXTURE` — поля зарезервированы
    // заранее, чтобы их появление в эдиторе в будущем не потребовало
    // версионирования этого формата.
    pub albedo_texture_id: u32,
    pub normal_texture_id: u32,
    pub metallic_roughness_texture_id: u32,
    pub ao_texture_id: u32,
    pub emissive_texture_id: u32,
}

/// Значение текстурного слота `MaterialDefinition`, означающее "слот не
/// используется" — тот же приём (максимум `u32` как "нет значения"), что
/// `GlobalObject::altex_file_id`/`ChunkObjectHeader` в других форматах
/// этого движка.
pub const NO_TEXTURE: u32 = 0xFFFF_FFFF;

pub struct AlmatFile {
    pub header: AlmatHeader,
    pub strings: Vec<String>,
    pub buckets: Vec<MaterialBucket>,
    pub materials: Vec<AcceleratedMaterial>,
    pub texture_atlas: Vec<TextureAtlasEntry>,
    /// ДОБАВЛЕНО: авторские материалы — см. подробный комментарий у
    /// `MaterialDefinition` про то, чем эта секция отличается от
    /// `materials` (`AcceleratedMaterial`) выше.
    pub material_definitions: Vec<MaterialDefinition>,
}

impl AlmatFile {
    pub fn new() -> Self {
        Self {
            header: AlmatHeader {
                magic: *b"ALKALMAT",
                version: 1,
                total_materials: 0,
                material_buckets: 4,
                string_table_offset: 0,
                bucket_table_offset: 0,
                material_table_offset: 0,
                texture_atlas_offset: 0,
                material_definitions_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            buckets: vec![
                MaterialBucket { bucket_type: 0, material_start: 0, material_count: 0, sort_key: 0 },
                MaterialBucket { bucket_type: 1, material_start: 0, material_count: 0, sort_key: 1 },
                MaterialBucket { bucket_type: 2, material_start: 0, material_count: 0, sort_key: 2 },
                MaterialBucket { bucket_type: 3, material_start: 0, material_count: 0, sort_key: 3 },
            ],
            materials: Vec::new(),
            texture_atlas: Vec::new(),
            material_definitions: Vec::new(),
        }
    }

    pub fn create_optimized() -> Self {
        let mut mat = AlmatFile::new();

        // Настройка для максимального батчинга
        mat.materials.push(AcceleratedMaterial {
            name_id: 0,
            shader_hash: 0xDEADBEEF,
            texture_handles: [0; 8],
            constant_buffer_data: [0; 16],
            render_state_hash: 0,
            batch_group: 0,
            lod_material_id: 0xFFFFFFFF,
            draw_calls_per_frame: 0,
        });

        mat
    }

    /// Переиспользует уже добавленную строку, если она совпадает — тот же
    /// приём, что и в остальных форматах этого движка (.alworld/.alfar/...).
    pub fn add_string(&mut self, s: &str) -> u32 {
        if let Some(pos) = self.strings.iter().position(|existing| existing == s) {
            return pos as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }

    pub fn get_string(&self, id: u32) -> &str {
        self.strings.get(id as usize).map(|s| s.as_str()).unwrap_or("")
    }

    /// Добавляет авторский материал по имени — `name_id` внутри `def`
    /// перезаписывается (вызывающему коду не нужно самому вызывать
    /// `add_string`), тот же паттерн, что `AlfarFile::add_light`.
    pub fn add_material_definition(&mut self, mut def: MaterialDefinition, name: &str) -> u32 {
        def.name_id = self.add_string(name);
        self.material_definitions.push(def);
        (self.material_definitions.len() - 1) as u32
    }

    // =====================================================================
    // ДОБАВЛЕНО (авторские материалы — по прямому запросу пользователя):
    // раньше `save()`/`load()` у `AlmatFile` не существовало ВООБЩЕ — формат
    // мог быть только создан в памяти (`new()`/`create_optimized()`), но
    // никогда не сохранялся и не читался с диска. Формат файла — тот же
    // общий стиль, что и у остальных форматов этого движка (header ->
    // string table (count + [len+bytes]) -> bucket table -> accelerated
    // material table -> texture atlas table -> material definition table),
    // каждая секция — count(u32) + POD-массив своей структуры.
    // =====================================================================
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(&(s.len() as u32).to_le_bytes());
            strings_data.extend_from_slice(s.as_bytes());
        }

        let mut buckets_data = Vec::new();
        buckets_data.extend_from_slice(&(self.buckets.len() as u32).to_le_bytes());
        for b in &self.buckets {
            buckets_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(b as *const MaterialBucket as *const u8, std::mem::size_of::<MaterialBucket>())
            });
        }

        let mut materials_data = Vec::new();
        materials_data.extend_from_slice(&(self.materials.len() as u32).to_le_bytes());
        for m in &self.materials {
            materials_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(m as *const AcceleratedMaterial as *const u8, std::mem::size_of::<AcceleratedMaterial>())
            });
        }

        let mut atlas_data = Vec::new();
        atlas_data.extend_from_slice(&(self.texture_atlas.len() as u32).to_le_bytes());
        for a in &self.texture_atlas {
            atlas_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(a as *const TextureAtlasEntry as *const u8, std::mem::size_of::<TextureAtlasEntry>())
            });
        }

        let mut defs_data = Vec::new();
        defs_data.extend_from_slice(&(self.material_definitions.len() as u32).to_le_bytes());
        for d in &self.material_definitions {
            defs_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(d as *const MaterialDefinition as *const u8, std::mem::size_of::<MaterialDefinition>())
            });
        }

        let header_size = std::mem::size_of::<AlmatHeader>() as u64;
        let string_table_offset = header_size;
        let bucket_table_offset = string_table_offset + strings_data.len() as u64;
        let material_table_offset = bucket_table_offset + buckets_data.len() as u64;
        let texture_atlas_offset = material_table_offset + materials_data.len() as u64;
        let material_definitions_offset = texture_atlas_offset + atlas_data.len() as u64;

        let header = AlmatHeader {
            magic: self.header.magic,
            version: self.header.version,
            total_materials: self.material_definitions.len() as u32,
            material_buckets: self.buckets.len() as u32,
            string_table_offset,
            bucket_table_offset,
            material_table_offset,
            texture_atlas_offset,
            material_definitions_offset,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlmatHeader as *const u8, std::mem::size_of::<AlmatHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&buckets_data)?;
        file.write_all(&materials_data)?;
        file.write_all(&atlas_data)?;
        file.write_all(&defs_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let header_size = std::mem::size_of::<AlmatHeader>();
        if buf.len() < header_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "almat: файл короче заголовка AlmatHeader",
            ));
        }

        // SAFETY: AlmatHeader — #[repr(C)], POD, длина буфера уже проверена.
        let header: AlmatHeader = unsafe {
            std::ptr::read_unaligned(buf.as_ptr() as *const AlmatHeader)
        };

        if &header.magic != b"ALKALMAT" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("almat: неверная сигнатура {:?}, ожидалось ALKALMAT", header.magic),
            ));
        }
        if header.version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("almat: неподдерживаемая версия формата {}", header.version),
            ));
        }

        let read_at = |offset: u64, size: usize, what: &str| -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.checked_add(size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("almat: переполнение при вычислении конца блока {}", what),
            ))?;
            buf.get(start..end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("almat: блок {} выходит за пределы файла (offset={}, size={}, file_len={})", what, offset, size, buf.len()),
            ))
        };

        // Строковая таблица: count(u32) + N раз [len(u32) + байты БЕЗ
        // null-терминатора] — тот же формат, что у .alworld/.alfar.
        let strings_start = header.string_table_offset as usize;
        if strings_start + 4 > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "almat: string_table_offset выходит за пределы файла",
            ));
        }
        let string_count = u32::from_le_bytes(buf[strings_start..strings_start + 4].try_into().unwrap()) as usize;
        let mut cursor = strings_start + 4;
        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            let len_bytes = read_at(cursor as u64, 4, "string length")?;
            let len = u32::from_le_bytes(len_bytes.try_into().unwrap()) as usize;
            cursor += 4;
            let str_bytes = read_at(cursor as u64, len, "string data")?;
            strings.push(String::from_utf8_lossy(str_bytes).into_owned());
            cursor += len;
        }

        // Универсальный помощник для секций вида count(u32) + POD-массив T
        // — четыре секции ниже (buckets/materials/atlas/definitions)
        // устроены одинаково, отличается только T и offset.
        fn read_pod_section<T: Copy>(
            buf: &[u8],
            offset: u64,
            what: &str,
        ) -> std::io::Result<Vec<T>> {
            let start = offset as usize;
            if start + 4 > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("almat: {}_offset выходит за пределы файла", what),
                ));
            }
            let count = u32::from_le_bytes(buf[start..start + 4].try_into().unwrap()) as usize;
            let item_size = std::mem::size_of::<T>();
            let items_start = start + 4;
            let items_end = items_start.checked_add(count * item_size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("almat: переполнение при вычислении конца блока {}", what),
            ))?;
            let items_bytes = buf.get(items_start..items_end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("almat: блок {} выходит за пределы файла (offset={}, count={}, file_len={})", what, offset, count, buf.len()),
            ))?;
            let mut items = Vec::with_capacity(count);
            for i in 0..count {
                let s = i * item_size;
                let item: T = unsafe {
                    std::ptr::read_unaligned(items_bytes[s..s + item_size].as_ptr() as *const T)
                };
                items.push(item);
            }
            Ok(items)
        }

        let buckets = read_pod_section::<MaterialBucket>(&buf, header.bucket_table_offset, "buckets")?;
        let materials = read_pod_section::<AcceleratedMaterial>(&buf, header.material_table_offset, "accelerated materials")?;
        let texture_atlas = read_pod_section::<TextureAtlasEntry>(&buf, header.texture_atlas_offset, "texture atlas")?;
        let material_definitions = read_pod_section::<MaterialDefinition>(&buf, header.material_definitions_offset, "material definitions")?;

        Ok(Self {
            header,
            strings,
            buckets,
            materials,
            texture_atlas,
            material_definitions,
        })
    }
}

impl Default for AlmatFile {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_material_definitions() {
        let mut mat = AlmatFile::new();
        mat.add_material_definition(
            MaterialDefinition {
                name_id: 0, // перезаписывается add_material_definition
                albedo: [0.8, 0.2, 0.1, 1.0],
                metallic: 0.3,
                roughness: 0.6,
                ao: 1.0,
                emissive: [0.0, 0.0, 0.0],
                albedo_texture_id: NO_TEXTURE,
                normal_texture_id: NO_TEXTURE,
                metallic_roughness_texture_id: NO_TEXTURE,
                ao_texture_id: NO_TEXTURE,
                emissive_texture_id: NO_TEXTURE,
            },
            "Rust",
        );
        mat.add_material_definition(
            MaterialDefinition {
                name_id: 0,
                albedo: [0.1, 0.1, 0.1, 1.0],
                metallic: 1.0,
                roughness: 0.1,
                ao: 1.0,
                emissive: [0.0, 0.5, 0.9],
                albedo_texture_id: NO_TEXTURE,
                normal_texture_id: NO_TEXTURE,
                metallic_roughness_texture_id: NO_TEXTURE,
                ao_texture_id: NO_TEXTURE,
                emissive_texture_id: NO_TEXTURE,
            },
            "Neon Metal",
        );

        let path = std::env::temp_dir().join("alkash3d_almat_roundtrip_test.almat");
        let path_str = path.to_string_lossy().to_string();
        mat.save(&path_str).expect("save");

        let loaded = AlmatFile::load(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.material_definitions.len(), 2);
        assert_eq!(loaded.header.total_materials, 2);

        let rust = &loaded.material_definitions[0];
        assert_eq!(loaded.get_string(rust.name_id), "Rust");
        assert!((rust.albedo[0] - 0.8).abs() < 1e-5);
        assert!((rust.metallic - 0.3).abs() < 1e-5);
        assert_eq!(rust.albedo_texture_id, NO_TEXTURE);

        let neon = &loaded.material_definitions[1];
        assert_eq!(loaded.get_string(neon.name_id), "Neon Metal");
        assert!((neon.emissive[2] - 0.9).abs() < 1e-5);
        assert!((neon.roughness - 0.1).abs() < 1e-5);
    }
}
