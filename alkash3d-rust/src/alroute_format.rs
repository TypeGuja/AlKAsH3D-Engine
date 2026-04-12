// alroute_format.rs - Полная исправленная версия

use std::io::{Read, Write, Seek, SeekFrom};

#[repr(C)]
pub struct AlrouteHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub route_count: u32,
    pub waypoint_count: u32,
    pub strings_offset: u64,
    pub routes_offset: u64,
    pub waypoints_offset: u64,
}

#[repr(C)]
pub struct Route {
    pub name_id: u32,
    pub waypoint_start: u32,
    pub waypoint_count: u32,
    pub loop_type: u32,
    pub speed_factor: f32,
    pub start_delay: f32,
}

#[repr(C)]
#[derive(Clone)]
pub struct Waypoint {
    pub position: [f32; 3],
    pub wait_time: f32,
    pub speed_limit: f32,
    pub action_id: u32,
}

pub struct AlrouteFile {
    pub header: AlrouteHeader,
    pub strings: Vec<String>,
    pub routes: Vec<Route>,
    pub waypoints: Vec<Waypoint>,
}

impl AlrouteFile {
    pub fn new() -> Self {
        Self {
            header: AlrouteHeader {
                magic: *b"ALKROUTE",
                version: 1,
                route_count: 0,
                waypoint_count: 0,
                strings_offset: 0,
                routes_offset: 0,
                waypoints_offset: 0,
            },
            strings: Vec::new(),
            routes: Vec::new(),
            waypoints: Vec::new(),
        }
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }

    pub fn add_route(&mut self, name: &str, waypoints: &[Waypoint], loop_type: u32) -> u32 {
        let name_id = self.add_string(name);
        let route_id = self.routes.len() as u32;

        self.routes.push(Route {
            name_id,
            waypoint_start: self.waypoints.len() as u32,
            waypoint_count: waypoints.len() as u32,
            loop_type,
            speed_factor: 1.0,
            start_delay: 0.0,
        });

        self.waypoints.extend(waypoints.iter().cloned());
        route_id
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(s.as_bytes());
            strings_data.push(0);
        }

        let mut routes_data = Vec::new();
        for route in &self.routes {
            routes_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(route as *const Route as *const u8, std::mem::size_of::<Route>())
            });
        }

        let mut waypoints_data = Vec::new();
        for wp in &self.waypoints {
            waypoints_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(wp as *const Waypoint as *const u8, std::mem::size_of::<Waypoint>())
            });
        }

        let header_size = std::mem::size_of::<AlrouteHeader>() as u64;
        let strings_offset = header_size;
        let routes_offset = strings_offset + strings_data.len() as u64;
        let waypoints_offset = routes_offset + routes_data.len() as u64;

        let header = AlrouteHeader {
            magic: self.header.magic,
            version: self.header.version,
            route_count: self.routes.len() as u32,
            waypoint_count: self.waypoints.len() as u32,
            strings_offset,
            routes_offset,
            waypoints_offset,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlrouteHeader as *const u8, std::mem::size_of::<AlrouteHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&routes_data)?;
        file.write_all(&waypoints_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut header_bytes = vec![0u8; std::mem::size_of::<AlrouteHeader>()];
        file.read_exact(&mut header_bytes)?;

        let header: AlrouteHeader = unsafe { std::ptr::read(header_bytes.as_ptr() as *const AlrouteHeader) };

        if &header.magic != b"ALKROUTE" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid .alroute file"));
        }

        let mut alroute = AlrouteFile::new();
        alroute.header = header;

        file.seek(SeekFrom::Start(alroute.header.strings_offset))?;
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
            alroute.strings.push(String::from_utf8_lossy(&buffer).to_string());
        }

        file.seek(SeekFrom::Start(alroute.header.routes_offset))?;
        for _ in 0..alroute.header.route_count {
            let mut route: Route = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut route as *mut Route as *mut u8, std::mem::size_of::<Route>())
            })?;
            alroute.routes.push(route);
        }

        file.seek(SeekFrom::Start(alroute.header.waypoints_offset))?;
        for _ in 0..alroute.header.waypoint_count {
            let mut wp: Waypoint = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut wp as *mut Waypoint as *mut u8, std::mem::size_of::<Waypoint>())
            })?;
            alroute.waypoints.push(wp);
        }

        Ok(alroute)
    }

    pub fn get_string(&self, id: u32) -> &str {
        &self.strings[id as usize]
    }
}