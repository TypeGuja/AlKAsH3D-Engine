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

        let header_size = std::mem::size_of::<AltexHeader>() as u64;
        let string_offset = header_size;
        let scene_offset = string_offset + strings_data.len() as u64;
        let geom_offset = scene_offset + scene_data.len() as u64;
        let total_size = geom_offset + geom_data.len() as u64;

        let header = AltexHeader {
            magic: self.header.magic,
            version: self.header.version,
            flags: self.header.flags,
            string_table_offset: string_offset,
            scene_offset: scene_offset,
            geometry_offset: geom_offset,
            total_size,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AltexHeader as *const u8, std::mem::size_of::<AltexHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&scene_data)?;
        file.write_all(&geom_data)?;

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

        Ok(altex)
    }

    pub fn get_string(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }
}