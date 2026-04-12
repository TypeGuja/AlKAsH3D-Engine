// alcar_format.rs - Car Archive

use std::io::{Read, Write, Seek, SeekFrom};

#[repr(C)]
pub struct AlcarHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub strings_offset: u64,
    pub mesh_offset: u64,
    pub physics_offset: u64,
    pub audio_offset: u64,
    pub lights_offset: u64,
    pub metadata_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
pub struct CarPhysics {
    pub engine_power: f32,
    pub torque: f32,
    pub max_rpm: f32,
    pub idle_rpm: f32,
    pub gears: u32,
    pub gear_ratios: [f32; 8],
    pub final_drive: f32,
    pub weight: f32,
    pub wheel_count: u32,
    pub wheel_radius: f32,
    pub suspension_stiffness: f32,
    pub suspension_damping: f32,
    pub brake_power: f32,
    pub handbrake_power: f32,
    pub steering_angle: f32,
    pub turning_radius: f32,
    pub drag_coefficient: f32,
    pub downforce: f32,
    pub collision_margin: f32,
    pub body_roll_factor: f32,
    pub top_speed: f32,
    pub acceleration_0_100: f32,
}

#[repr(C)]
pub struct CarAudio {
    pub engine_start_id: u32,
    pub engine_idle_id: u32,
    pub engine_accel_id: u32,
    pub engine_decel_id: u32,
    pub horn_id: u32,
    pub siren_id: u32,
    pub crash_id: u32,
    pub skid_id: u32,
    pub engine_pitch_min: f32,
    pub engine_pitch_max: f32,
    pub engine_volume_min: f32,
    pub engine_volume_max: f32,
}

#[repr(C)]
pub struct CarLights {
    pub headlight_count: u32,
    pub headlights: [CarLight; 4],
    pub taillight_count: u32,
    pub taillights: [CarLight; 4],
    pub blinker_count: u32,
    pub blinkers: [CarLight; 4],
    pub has_siren: u32,
    pub siren_lights: [CarLight; 4],
    pub siren_light_count: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]  // Добавь эту строку
pub struct CarLight {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub cone_angle: f32,
}

#[repr(C)]
pub struct CarMetadata {
    pub brand_id: u32,
    pub model_id: u32,
    pub year: u32,
    pub price: u32,
    pub fuel_consumption: f32,
    pub fuel_tank: f32,
    pub category: u32,
    pub rarity: u32,
    pub ai_script_id: u32,
}

pub struct AlcarFile {
    pub header: AlcarHeader,
    pub strings: Vec<String>,
    pub mesh_path: String,
    pub textures: Vec<String>,
    pub physics: CarPhysics,
    pub audio: CarAudio,
    pub lights: CarLights,
    pub metadata: CarMetadata,
    pub custom_data: Vec<u8>,
}

impl Default for CarLight {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0, 0.0],
            color: [1.0, 1.0, 1.0],
            intensity: 1.0,
            range: 10.0,
            cone_angle: 30.0,
        }
    }
}

impl AlcarFile {
    pub fn new() -> Self {
        Self {
            header: AlcarHeader {
                magic: *b"ALKALCAR",
                version: 1,
                flags: 0,
                strings_offset: 0,
                mesh_offset: 0,
                physics_offset: 0,
                audio_offset: 0,
                lights_offset: 0,
                metadata_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            mesh_path: String::new(),
            textures: Vec::new(),
            physics: CarPhysics {
                engine_power: 150.0,
                torque: 200.0,
                max_rpm: 6500.0,
                idle_rpm: 800.0,
                gears: 6,
                gear_ratios: [3.5, 2.0, 1.4, 1.0, 0.8, 0.6, 0.0, 0.0],
                final_drive: 3.5,
                weight: 1500.0,
                wheel_count: 4,
                wheel_radius: 0.33,
                suspension_stiffness: 25000.0,
                suspension_damping: 2000.0,
                brake_power: 8000.0,
                handbrake_power: 5000.0,
                steering_angle: 35.0,
                turning_radius: 5.5,
                drag_coefficient: 0.3,
                downforce: 50.0,
                collision_margin: 0.2,
                body_roll_factor: 0.5,
                top_speed: 220.0,
                acceleration_0_100: 8.5,
            },
            audio: CarAudio {
                engine_start_id: 0,
                engine_idle_id: 0,
                engine_accel_id: 0,
                engine_decel_id: 0,
                horn_id: 0,
                siren_id: 0,
                crash_id: 0,
                skid_id: 0,
                engine_pitch_min: 0.5,
                engine_pitch_max: 1.5,
                engine_volume_min: 0.3,
                engine_volume_max: 1.0,
            },
            lights: CarLights {
                headlight_count: 2,
                headlights: [CarLight::default(); 4],
                taillight_count: 2,
                taillights: [CarLight::default(); 4],
                blinker_count: 4,
                blinkers: [CarLight::default(); 4],
                has_siren: 0,
                siren_lights: [CarLight::default(); 4],
                siren_light_count: 0,
            },
            metadata: CarMetadata {
                brand_id: 0,
                model_id: 0,
                year: 2024,
                price: 25000,
                fuel_consumption: 8.0,
                fuel_tank: 55.0,
                category: 0,
                rarity: 0,
                ai_script_id: 0,
            },
            custom_data: Vec::new(),
        }
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }

    pub fn set_mesh(&mut self, path: &str) {
        self.mesh_path = path.to_string();
    }

    pub fn create_police_car() -> Self {
        let mut car = AlcarFile::new();
        car.metadata.category = 4;
        car.metadata.ai_script_id = car.add_string("police_ai.py");
        car.lights.has_siren = 1;
        car.lights.siren_light_count = 2;
        car.physics.engine_power = 300.0;
        car.physics.top_speed = 250.0;
        car
    }

    pub fn create_sports_car() -> Self {
        let mut car = AlcarFile::new();
        car.metadata.category = 2;
        car.physics.engine_power = 500.0;
        car.physics.gears = 7;
        car.physics.acceleration_0_100 = 3.5;
        car.physics.top_speed = 320.0;
        car
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(s.as_bytes());
            strings_data.push(0);
        }

        let mut mesh_data = Vec::new();
        let mesh_path_bytes = self.mesh_path.as_bytes();
        mesh_data.extend_from_slice(&(mesh_path_bytes.len() as u32).to_le_bytes());
        mesh_data.extend_from_slice(mesh_path_bytes);
        mesh_data.push(0);
        mesh_data.extend_from_slice(&(self.textures.len() as u32).to_le_bytes());
        for tex in &self.textures {
            let tex_bytes = tex.as_bytes();
            mesh_data.extend_from_slice(&(tex_bytes.len() as u32).to_le_bytes());
            mesh_data.extend_from_slice(tex_bytes);
            mesh_data.push(0);
        }

        let physics_data = unsafe {
            std::slice::from_raw_parts(&self.physics as *const CarPhysics as *const u8, std::mem::size_of::<CarPhysics>())
        };
        let audio_data = unsafe {
            std::slice::from_raw_parts(&self.audio as *const CarAudio as *const u8, std::mem::size_of::<CarAudio>())
        };
        let lights_data = unsafe {
            std::slice::from_raw_parts(&self.lights as *const CarLights as *const u8, std::mem::size_of::<CarLights>())
        };
        let metadata_data = unsafe {
            std::slice::from_raw_parts(&self.metadata as *const CarMetadata as *const u8, std::mem::size_of::<CarMetadata>())
        };

        let header_size = std::mem::size_of::<AlcarHeader>() as u64;
        let strings_offset = header_size;
        let mesh_offset = strings_offset + strings_data.len() as u64;
        let physics_offset = mesh_offset + mesh_data.len() as u64;
        let audio_offset = physics_offset + physics_data.len() as u64;
        let lights_offset = audio_offset + audio_data.len() as u64;
        let metadata_offset = lights_offset + lights_data.len() as u64;

        let header = AlcarHeader {
            magic: self.header.magic,
            version: self.header.version,
            flags: self.header.flags,
            strings_offset,
            mesh_offset,
            physics_offset,
            audio_offset,
            lights_offset,
            metadata_offset,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlcarHeader as *const u8, std::mem::size_of::<AlcarHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&mesh_data)?;
        file.write_all(physics_data)?;
        file.write_all(audio_data)?;
        file.write_all(lights_data)?;
        file.write_all(metadata_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut header_bytes = vec![0u8; std::mem::size_of::<AlcarHeader>()];
        file.read_exact(&mut header_bytes)?;

        let header: AlcarHeader = unsafe { std::ptr::read(header_bytes.as_ptr() as *const AlcarHeader) };

        if &header.magic != b"ALKALCAR" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid .alcar file"));
        }

        let mut alcar = AlcarFile::new();
        alcar.header = header;

        file.seek(SeekFrom::Start(alcar.header.strings_offset))?;
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
            alcar.strings.push(String::from_utf8_lossy(&buffer).to_string());
        }

        file.seek(SeekFrom::Start(alcar.header.mesh_offset))?;
        let mut path_len_bytes = [0u8; 4];
        file.read_exact(&mut path_len_bytes)?;
        let path_len = u32::from_le_bytes(path_len_bytes) as usize;
        let mut path_bytes = vec![0u8; path_len];
        file.read_exact(&mut path_bytes)?;
        let mut null = [0u8; 1];
        file.read_exact(&mut null)?;
        alcar.mesh_path = String::from_utf8_lossy(&path_bytes).to_string();

        let mut tex_count_bytes = [0u8; 4];
        file.read_exact(&mut tex_count_bytes)?;
        let tex_count = u32::from_le_bytes(tex_count_bytes);
        for _ in 0..tex_count {
            let mut tex_len_bytes = [0u8; 4];
            file.read_exact(&mut tex_len_bytes)?;
            let tex_len = u32::from_le_bytes(tex_len_bytes) as usize;
            let mut tex_bytes = vec![0u8; tex_len];
            file.read_exact(&mut tex_bytes)?;
            file.read_exact(&mut null)?;
            alcar.textures.push(String::from_utf8_lossy(&tex_bytes).to_string());
        }

        file.seek(SeekFrom::Start(alcar.header.physics_offset))?;
        let mut physics: CarPhysics = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut physics as *mut CarPhysics as *mut u8, std::mem::size_of::<CarPhysics>())
        })?;
        alcar.physics = physics;

        file.seek(SeekFrom::Start(alcar.header.audio_offset))?;
        let mut audio: CarAudio = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut audio as *mut CarAudio as *mut u8, std::mem::size_of::<CarAudio>())
        })?;
        alcar.audio = audio;

        file.seek(SeekFrom::Start(alcar.header.lights_offset))?;
        let mut lights: CarLights = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut lights as *mut CarLights as *mut u8, std::mem::size_of::<CarLights>())
        })?;
        alcar.lights = lights;

        file.seek(SeekFrom::Start(alcar.header.metadata_offset))?;
        let mut metadata: CarMetadata = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut metadata as *mut CarMetadata as *mut u8, std::mem::size_of::<CarMetadata>())
        })?;
        alcar.metadata = metadata;

        Ok(alcar)
    }

    pub fn get_string(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }
}