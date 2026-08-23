// altex_format.rs - Полная версия

use std::io::{Read, Write, Seek, SeekFrom};

#[repr(C)]
pub struct AltexHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub string_table_offset: u64,
    pub scene_offset: u64,
    pub geometry_offset: u64,
    pub total_size: u64,
    pub created_at: u64,
    // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Раньше
    // `materials`/`textures`/`texture_data` существовали как ПОЛЯ
    // `AltexFile` (см. ниже), но НИКОГДА не сериализовались save()/load()
    // — то есть в РЕАЛЬНОСТИ ни один .altex файл на диске не мог нести
    // материал/текстуру, даже если код-экспортёр их заполнил перед
    // вызовом save(). Поле добавлено В КОНЕЦ структуры (после
    // существующих полей, не между ними) — это НЕ ломает бинарную
    // совместимость чтения: `load()` ниже читает материалы ТОЛЬКО когда
    // `version >= 2 && materials_offset != 0`, старые файлы версии 1
    // (materials_offset была бы мусором за пределами реально записанных
    // байт заголовка) для этого условия не подходят и получают пустые
    // materials/textures/texture_data — так же, как получали раньше.
    pub materials_offset: u64,
}

#[repr(C)]
pub struct SceneHeader {
    pub object_count: u32,
    pub instance_count: u32,
}

#[repr(C)]
pub struct SceneObject {
    pub name_id: u32,
    pub mesh_id: u32,
    pub material_id: u32,
    pub transform_id: u32,
    pub flags: u32,
    pub custom_data_offset: u64,
}

#[repr(C)]
pub struct Transform {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[repr(C)]
pub struct GeometryHeader {
    pub mesh_count: u32,
    pub total_vertices: u32,
    pub total_indices: u32,
    pub vertex_format: u32,
}

#[repr(C)]
pub struct Mesh {
    pub name_id: u32,
    pub vertex_offset: u32,
    pub index_offset: u32,
    pub vertex_count: u32,
    pub index_count: u32,
    pub material_id: u32,
    pub min_bound: [f32; 3],
    pub max_bound: [f32; 3],
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
    pub bitangent: [f32; 3],
    pub uv: [f32; 2],
    pub uv2: [f32; 2],
    pub color: [f32; 4],
}

impl Vertex {
    pub const STRIDE: usize = std::mem::size_of::<Vertex>();
}

#[repr(C)]
pub struct Material {
    pub name_id: u32,
    pub shader_id: u32,
    pub albedo: [f32; 4],
    pub metallic: f32,
    pub roughness: f32,
    pub ao: f32,
    pub emissive: [f32; 3],
    pub albedo_map: u32,
    pub normal_map: u32,
    pub metallic_map: u32,
    pub roughness_map: u32,
    pub ao_map: u32,
    pub emissive_map: u32,
}

#[repr(C)]
pub struct Texture {
    pub name_id: u32,
    pub width: u32,
    pub height: u32,
    pub mip_levels: u32,
    pub format: u32,
    pub data_offset: u64,
    pub data_size: u64,
}

pub struct AltexFile {
    pub header: AltexHeader,
    pub strings: Vec<String>,
    pub objects: Vec<SceneObject>,
    pub transforms: Vec<Transform>,
    pub meshes: Vec<Mesh>,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub materials: Vec<Material>,
    pub textures: Vec<Texture>,
    pub texture_data: Vec<u8>,
}

impl AltexFile {
    pub fn new() -> Self {
        Self {
            header: AltexHeader {
                magic: *b"ALKALTEX",
                version: 1,
                flags: 0,
                string_table_offset: 0,
                scene_offset: 0,
                geometry_offset: 0,
                total_size: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
                materials_offset: 0,
            },
            strings: Vec::new(),
            objects: Vec::new(),
            transforms: Vec::new(),
            meshes: Vec::new(),
            vertices: Vec::new(),
            indices: Vec::new(),
            materials: Vec::new(),
            textures: Vec::new(),
            texture_data: Vec::new(),
        }
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }

    pub fn add_mesh(&mut self, vertices: Vec<Vertex>, indices: Vec<u32>, name: &str) -> u32 {
        let mesh_id = self.meshes.len() as u32;
        let name_id = self.add_string(name);
        let vertex_offset = self.vertices.len() as u32;
        let index_offset = self.indices.len() as u32;
        let vertex_count = vertices.len() as u32;
        let index_count = indices.len() as u32;

        self.meshes.push(Mesh {
            name_id,
            vertex_offset,
            index_offset,
            vertex_count,
            index_count,
            material_id: 0xFFFFFFFF,
            min_bound: [0.0; 3],
            max_bound: [0.0; 3],
        });

        self.vertices.extend(vertices);
        self.indices.extend(indices);
        mesh_id
    }

    /// ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Добавляет
    /// текстуру (сырые пиксельные байты — RGBA8, см. `format`/`width`/
    /// `height`, интерпретация формата — на совести загружающего кода,
    /// сам `.altex` формат её не проверяет и не конвертирует) в общий пул
    /// `texture_data` файла и регистрирует её метаданные в `textures`.
    /// Возвращает индекс новой текстуры в `self.textures` — именно этот
    /// индекс идёт в `Material::albedo_map`/`normal_map`/и т.д.
    pub fn add_texture(&mut self, width: u32, height: u32, format: u32, pixels: &[u8], name: &str) -> u32 {
        let texture_id = self.textures.len() as u32;
        let name_id = self.add_string(name);
        let data_offset = self.texture_data.len() as u64;
        let data_size = pixels.len() as u64;

        self.textures.push(Texture {
            name_id,
            width,
            height,
            mip_levels: 1,
            format,
            data_offset,
            data_size,
        });

        self.texture_data.extend_from_slice(pixels);
        texture_id
    }

    /// ДОБАВЛЕНО (Задача #15): добавляет материал — `albedo_map` (и
    /// прочие *_map поля) должны быть либо валидным индексом в
    /// `self.textures` (см. `add_texture`), либо `0xFFFFFFFF` ("карты
    /// нет, использовать только скалярное значение `albedo`/`metallic`/
    /// и т.п." — тот же sentinel-паттерн, что уже применён у
    /// `Mesh::material_id`/`SceneObject::material_id` выше).
    pub fn add_material(&mut self, albedo: [f32; 4], albedo_map: u32, name: &str) -> u32 {
        let material_id = self.materials.len() as u32;
        let name_id = self.add_string(name);

        self.materials.push(Material {
            name_id,
            shader_id: 0,
            albedo,
            metallic: 0.0,
            roughness: 0.8,
            ao: 1.0,
            emissive: [0.0, 0.0, 0.0],
            albedo_map,
            normal_map: 0xFFFFFFFF,
            metallic_map: 0xFFFFFFFF,
            roughness_map: 0xFFFFFFFF,
            ao_map: 0xFFFFFFFF,
            emissive_map: 0xFFFFFFFF,
        });

        material_id
    }

    pub fn add_object(&mut self, mesh_id: u32, transform: Transform, name: &str) -> u32 {
        let obj_id = self.objects.len() as u32;
        let transform_id = self.add_transform(transform);
        let name_id = self.add_string(name);

        self.objects.push(SceneObject {
            name_id,
            mesh_id,
            material_id: 0xFFFFFFFF,
            transform_id,
            flags: 1,
            custom_data_offset: 0,
        });

        obj_id
    }

    fn add_transform(&mut self, transform: Transform) -> u32 {
        let id = self.transforms.len() as u32;
        self.transforms.push(transform);
        id
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(s.as_bytes());
            strings_data.push(0);
        }

        let mut scene_data = Vec::new();
        let scene_header = SceneHeader {
            object_count: self.objects.len() as u32,
            instance_count: 0,
        };
        scene_data.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&scene_header as *const SceneHeader as *const u8, std::mem::size_of::<SceneHeader>())
        });

        for obj in &self.objects {
            scene_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(obj as *const SceneObject as *const u8, std::mem::size_of::<SceneObject>())
            });
        }

        for transform in &self.transforms {
            scene_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(transform as *const Transform as *const u8, std::mem::size_of::<Transform>())
            });
        }

        let mut geom_data = Vec::new();
        let geom_header = GeometryHeader {
            mesh_count: self.meshes.len() as u32,
            total_vertices: self.vertices.len() as u32,
            total_indices: self.indices.len() as u32,
            vertex_format: 0,
        };
        geom_data.extend_from_slice(unsafe {
            std::slice::from_raw_parts(&geom_header as *const GeometryHeader as *const u8, std::mem::size_of::<GeometryHeader>())
        });

        for mesh in &self.meshes {
            geom_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(mesh as *const Mesh as *const u8, std::mem::size_of::<Mesh>())
            });
        }

        for vertex in &self.vertices {
            geom_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(vertex as *const Vertex as *const u8, Vertex::STRIDE)
            });
        }

        for index in &self.indices {
            geom_data.extend_from_slice(&index.to_le_bytes());
        }

        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Блок
        // материалов/текстур — тот же общий стиль "count-header + POD-
        // массив", что и у geom_data выше: count материалов, затем сами
        // Material (POD), затем count текстур, затем сами Texture (POD,
        // метаданные БЕЗ пиксельных данных), затем размер + сырые байты
        // texture_data ОДНИМ блоком (не по одной текстуре — Texture::
        // data_offset/data_size уже дают точные границы каждой текстуры
        // внутри общего texture_data, отдельно нарезать на диске нет
        // смысла).
        let mut materials_data = Vec::new();
        materials_data.extend_from_slice(&(self.materials.len() as u32).to_le_bytes());
        for material in &self.materials {
            materials_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(material as *const Material as *const u8, std::mem::size_of::<Material>())
            });
        }
        materials_data.extend_from_slice(&(self.textures.len() as u32).to_le_bytes());
        for texture in &self.textures {
            materials_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(texture as *const Texture as *const u8, std::mem::size_of::<Texture>())
            });
        }
        materials_data.extend_from_slice(&(self.texture_data.len() as u64).to_le_bytes());
        materials_data.extend_from_slice(&self.texture_data);

        let header_size = std::mem::size_of::<AltexHeader>() as u64;
        let string_offset = header_size;
        let scene_offset = string_offset + strings_data.len() as u64;
        let geom_offset = scene_offset + scene_data.len() as u64;
        // materials_offset ставим ВСЕГДА (не только когда материалы
        // реально есть) — `load()` определяет "материалов нет" по
        // count==0 внутри самого блока, а не по нулевому офсету; нулевой
        // офсет как раз ЗАРЕЗЕРВИРОВАН как признак "файл версии 1, блока
        // вообще нет на диске" (см. `load()`).
        let materials_offset = geom_offset + geom_data.len() as u64;
        let total_size = materials_offset + materials_data.len() as u64;

        // ДОБАВЛЕНО (Задача #15): version=2 — сигнализирует load(), что
        // materials_offset действительно указывает на реальный блок (а не
        // на мусор за пределами файла версии 1, у которой этого поля
        // физически не было). См. подробный комментарий у AltexHeader.
        let header = AltexHeader {
            magic: self.header.magic,
            version: 2,
            flags: self.header.flags,
            string_table_offset: string_offset,
            scene_offset: scene_offset,
            geometry_offset: geom_offset,
            total_size,
            created_at: self.header.created_at,
            materials_offset,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AltexHeader as *const u8, std::mem::size_of::<AltexHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&scene_data)?;
        file.write_all(&geom_data)?;
        file.write_all(&materials_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut header_bytes = vec![0u8; std::mem::size_of::<AltexHeader>()];
        file.read_exact(&mut header_bytes)?;

        let header: AltexHeader = unsafe { std::ptr::read(header_bytes.as_ptr() as *const AltexHeader) };

        if &header.magic != b"ALKALTEX" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid .altex file"));
        }

        let mut altex = AltexFile::new();
        altex.header = header;

        file.seek(SeekFrom::Start(altex.header.string_table_offset))?;
        let mut count_bytes = [0u8; 4];
        file.read_exact(&mut count_bytes)?;
        let str_count = u32::from_le_bytes(count_bytes);

        let mut buffer = Vec::new();
        for _ in 0..str_count {
            buffer.clear();
            let mut byte = [0u8; 1];
            loop {
                file.read_exact(&mut byte)?;
                if byte[0] == 0 { break; }
                buffer.push(byte[0]);
            }
            altex.strings.push(String::from_utf8_lossy(&buffer).to_string());
        }

        file.seek(SeekFrom::Start(altex.header.scene_offset))?;
        let mut scene_header: SceneHeader = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut scene_header as *mut SceneHeader as *mut u8, std::mem::size_of::<SceneHeader>())
        })?;

        for _ in 0..scene_header.object_count {
            let mut obj: SceneObject = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut obj as *mut SceneObject as *mut u8, std::mem::size_of::<SceneObject>())
            })?;
            altex.objects.push(obj);
        }

        for _ in 0..scene_header.object_count {
            let mut transform: Transform = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut transform as *mut Transform as *mut u8, std::mem::size_of::<Transform>())
            })?;
            altex.transforms.push(transform);
        }

        file.seek(SeekFrom::Start(altex.header.geometry_offset))?;
        let mut geom_header: GeometryHeader = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut geom_header as *mut GeometryHeader as *mut u8, std::mem::size_of::<GeometryHeader>())
        })?;

        for _ in 0..geom_header.mesh_count {
            let mut mesh: Mesh = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut mesh as *mut Mesh as *mut u8, std::mem::size_of::<Mesh>())
            })?;
            altex.meshes.push(mesh);
        }

        for _ in 0..geom_header.total_vertices {
            let mut vertex: Vertex = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut vertex as *mut Vertex as *mut u8, Vertex::STRIDE)
            })?;
            altex.vertices.push(vertex);
        }

        for _ in 0..geom_header.total_indices {
            let mut index_bytes = [0u8; 4];
            file.read_exact(&mut index_bytes)?;
            altex.indices.push(u32::from_le_bytes(index_bytes));
        }

        // ДОБАВЛЕНО (Задача #15: текстуры и PBR-материалы). Блок
        // материалов/текстур существует ТОЛЬКО в файлах version>=2 (см.
        // подробный комментарий у `AltexHeader::materials_offset` и у
        // `save()` выше) — файлы version 1 просто не имеют этих байт на
        // диске вообще, попытка их прочитать увела бы `file.seek` за
        // пределы файла и следующий `read_exact` вернул бы UnexpectedEof.
        // Пустые Vec (уже установлены `AltexFile::new()` в самом начале
        // этой функции) — корректный, ожидаемый результат для такого
        // файла, а не ошибка.
        if altex.header.version >= 2 && altex.header.materials_offset != 0 {
            file.seek(SeekFrom::Start(altex.header.materials_offset))?;

            let mut material_count_bytes = [0u8; 4];
            file.read_exact(&mut material_count_bytes)?;
            let material_count = u32::from_le_bytes(material_count_bytes);
            for _ in 0..material_count {
                let mut material: Material = unsafe { std::mem::zeroed() };
                file.read_exact(unsafe {
                    std::slice::from_raw_parts_mut(&mut material as *mut Material as *mut u8, std::mem::size_of::<Material>())
                })?;
                altex.materials.push(material);
            }

            let mut texture_count_bytes = [0u8; 4];
            file.read_exact(&mut texture_count_bytes)?;
            let texture_count = u32::from_le_bytes(texture_count_bytes);
            for _ in 0..texture_count {
                let mut texture: Texture = unsafe { std::mem::zeroed() };
                file.read_exact(unsafe {
                    std::slice::from_raw_parts_mut(&mut texture as *mut Texture as *mut u8, std::mem::size_of::<Texture>())
                })?;
                altex.textures.push(texture);
            }

            let mut texture_data_len_bytes = [0u8; 8];
            file.read_exact(&mut texture_data_len_bytes)?;
            let texture_data_len = u64::from_le_bytes(texture_data_len_bytes) as usize;
            let mut texture_data = vec![0u8; texture_data_len];
            file.read_exact(&mut texture_data)?;
            altex.texture_data = texture_data;
        }

        Ok(altex)
    }

    pub fn get_string(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }
}