// src/alasm_format.rs - Assembly Archive
//! .alasm — граф разбираемых деталей (машина/двигатель/коробка передач
//! и т.д.), формат для задачи "разобрать всё на части" из плана по
//! реализму. НЕЗАВИСИМ от `.alcar` (тот описывает ИГРОВЫЕ характеристики
//! машины — мощность/звук/фары/цену, см. `alcar_format.rs`; этот — из
//! КАКИХ ФИЗИЧЕСКИХ ДЕТАЛЕЙ она состоит и как они скреплены). Ничего в
//! `.alcar` не меняется этим файлом — они самостоятельны, полноценная
//! машина в будущем сможет ссылаться на `.alasm` через `custom_data` (уже
//! существующее в `AlcarFile` свободное поле), но эта связь — отдельная,
//! более поздняя задача.
//!
//! КЛЮЧЕВАЯ ИДЕЯ (то, ради чего это отдельный формат, а не просто список
//! мешей у машины): дерево `PartRecord` — не просто визуальная иерархия
//! (как `Scene::set_parent`), а ФИЗИЧЕСКАЯ — каждая деталь описывает,
//! КАКИМ типом соединения (см. `crate::plugin::joint_type`) она держится
//! за родителя и при какой нагрузке отламывается. Спавнящий код
//! (`engine/assembly.rs`) создаёт РЕАЛЬНОЕ физическое тело на каждую
//! деталь и РЕАЛЬНЫЙ джойнт `alkash3d-inertial` между ней и родителем —
//! то есть открутить/оторвать деталь означает вызвать `remove_constraint`
//! или довести соединение до `is_broken` через фактическую физику, а не
//! проиграть заранее срежиссированную анимацию.
//!
//! ПЕРЕИСПОЛЬЗОВАНИЕ (требование "не только машины, но и двигатели/
//! коробки передач по отдельности" — отечественный автопром): деталь
//! может ссылаться на ДРУГОЙ `.alasm`-файл целиком (`sub_assembly_path_id`
//! вместо `mesh_path_id`) — движок рекурсивно грузит его и подвешивает
//! его КОРЕНЬ к текущему родителю через joint-поля этой записи. Так один
//! и тот же "двигатель-ВАЗ-2106.alasm" одновременно: (1) один узел внутри
//! "ВАЗ-2106.alasm" (снят с машины — отделяется от неё как единое целое),
//! и (2) самостоятельный объект, если его загрузить напрямую (лежит на
//! верстаке — сам разбирается на поршни/коленвал/ГБЦ по тем же правилам).

use std::io::{Read, Write, Seek, SeekFrom};

/// Сентинел "нет строки"/"нет ссылки" для всех `*_id`-полей ниже —
/// `u32::MAX` вместо `Option<u32>`, чтобы `PartRecord`/`AssemblyMetadata`
/// оставались `#[repr(C)]` POD-структурами, пригодными для сырого
/// побайтового чтения/записи (тот же приём, что уже использует
/// `alcar_format.rs`/`alfar_format.rs` во всём проекте).
pub const NONE_ID: u32 = u32::MAX;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AlasmHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub strings_offset: u64,
    pub parts_offset: u64,
    pub metadata_offset: u64,
    pub created_at: u64,
}

/// Категория сборки — влияет ТОЛЬКО на то, как игра представляет объект
/// игроку (иконка/меню разборки), сама структура частей/физика одинакова
/// для любой категории. `Engine`/`Gearbox`/`Differential` существуют
/// отдельно от `Vehicle` именно для сценария "разобрать двигатель без
/// машины вокруг" — см. шапку файла.
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssemblyCategory {
    Generic = 0,
    Vehicle = 1,
    Engine = 2,
    Gearbox = 3,
    Differential = 4,
}

impl AssemblyCategory {
    fn from_u32(v: u32) -> Self {
        match v {
            1 => AssemblyCategory::Vehicle,
            2 => AssemblyCategory::Engine,
            3 => AssemblyCategory::Gearbox,
            4 => AssemblyCategory::Differential,
            _ => AssemblyCategory::Generic,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AssemblyMetadata {
    /// См. `AssemblyCategory` — хранится как `u32`, а не как enum
    /// напрямую, чтобы структура оставалась побайтово читаемой из файла
    /// без риска UB на непредвиденном значении (см. `category()` ниже,
    /// которая безопасно нормализует любое значение через `from_u32`).
    pub category: u32,
    pub name_id: u32,
    pub brand_id: u32,
    pub model_id: u32,
    pub year: u32,
}

impl AssemblyMetadata {
    pub fn category(&self) -> AssemblyCategory {
        AssemblyCategory::from_u32(self.category)
    }
}

impl Default for AssemblyMetadata {
    fn default() -> Self {
        Self {
            category: AssemblyCategory::Generic as u32,
            name_id: NONE_ID,
            brand_id: NONE_ID,
            model_id: NONE_ID,
            year: 0,
        }
    }
}

/// Одна деталь сборки — узел дерева. `parent_index` — индекс родителя в
/// `AlasmFile::parts` ЭТОГО ЖЕ файла (`-1` у РОВНО ОДНОЙ детали — корень,
/// например кузов машины или блок цилиндров двигателя — то, что "несёт"
/// на себе всё остальное и обычно остаётся динамическим телом даже после
/// полной разборки всего навесного).
///
/// Joint-поля (`joint_type`/`anchor_a`/`anchor_b`/`axis`/`break_impulse_*`)
/// описывают соединение МЕЖДУ ЭТОЙ деталью и её `parent_index` — см.
/// `crate::plugin::{joint_type, ConstraintDesc}`, поля здесь напрямую
/// копируются в `ConstraintDesc` при спавне (`anchor_a` — смещение точки
/// крепления от центра РОДИТЕЛЯ, `anchor_b` — от центра ЭТОЙ детали, тот
/// же порядок, что у `ConstraintDesc::body_a`/`body_b`, где `body_a` —
/// родитель). Игнорируются (но всё равно сохраняются в файле нулями) для
/// корневой детали — ей не за что крепиться внутри своей же сборки.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PartRecord {
    pub parent_index: i32,
    /// См. `crate::plugin::joint_type` (BALL=0/HINGE=1/FIXED=2/SLIDER=3).
    pub joint_type: i32,
    /// Положение центра детали ОТНОСИТЕЛЬНО РОДИТЕЛЯ (или в мировых
    /// координатах точки спавна — для корня) в состоянии "всё собрано".
    /// Используется и для визуального размещения при спавне, и как
    /// исходная точка перед тем, как физика/джойнты начнут её удерживать.
    pub local_position: [f32; 3],
    pub local_rotation: [f32; 3],
    pub mass: f32,
    pub friction: f32,
    pub restitution: f32,
    pub anchor_a: [f32; 3],
    pub anchor_b: [f32; 3],
    pub axis: [f32; 3],
    pub break_impulse_linear: f32,
    pub break_impulse_angular: f32,
    /// Индекс пути к `.altex`-геометрии в `AlasmFile::strings`, либо
    /// `NONE_ID` — тогда спавнящий код рисует плейсхолдер (см.
    /// `world_streaming.rs::load_placeholder_mesh` за прецедентом того же
    /// принципа "нет геометрии — не крах, а куб-заглушка"). Игнорируется,
    /// если `sub_assembly_path_id != NONE_ID` (см. ниже).
    pub mesh_path_id: u32,
    /// Если не `NONE_ID` — эта деталь НЕ обычная деталь, а ССЫЛКА на
    /// другой `.alasm` файл (путь — строка в `strings`, ОТНОСИТЕЛЬНО того
    /// же базового каталога, что и сам этот файл). `mesh_path_id`
    /// игнорируется в этом случае — при спавне движок рекурсивно грузит
    /// указанный файл и подвешивает ЕГО корень к текущему родителю через
    /// joint-поля ЭТОЙ записи (см. подробности в шапке файла).
    pub sub_assembly_path_id: u32,
    /// Человекочитаемое имя детали (индекс в `strings`) — для UI разборки
    /// ("Крышка клапанов", "Генератор"), `NONE_ID` допустим (безымянная
    /// деталь всё равно физически разбираема).
    pub name_id: u32,
    /// Индекс в `strings` — какой инструмент нужен, чтобы открутить эту
    /// деталь руками, а не силой (например "wrench_10", "wrench_13") —
    /// поле зарезервировано под БУДУЩИЙ игровой инструмент/инвентарь,
    /// сама физика разборки (constraint + break_impulse_*) от него не
    /// зависит. `NONE_ID` = деталь снимается голыми руками/не снимается
    /// иначе как поломкой.
    pub required_tool_id: u32,
}

impl Default for PartRecord {
    fn default() -> Self {
        Self {
            parent_index: -1,
            joint_type: 2, // FIXED — см. crate::plugin::joint_type::FIXED
            local_position: [0.0; 3],
            local_rotation: [0.0; 3],
            mass: 1.0,
            friction: 0.5,
            restitution: 0.1,
            anchor_a: [0.0; 3],
            anchor_b: [0.0; 3],
            axis: [0.0, 1.0, 0.0],
            break_impulse_linear: 0.0,
            break_impulse_angular: 0.0,
            mesh_path_id: NONE_ID,
            sub_assembly_path_id: NONE_ID,
            name_id: NONE_ID,
            required_tool_id: NONE_ID,
        }
    }
}

pub struct AlasmFile {
    pub header: AlasmHeader,
    pub strings: Vec<String>,
    pub parts: Vec<PartRecord>,
    pub metadata: AssemblyMetadata,
}

impl AlasmFile {
    pub fn new(name: &str, category: AssemblyCategory) -> Self {
        let mut file = Self {
            header: AlasmHeader {
                magic: *b"ALKALASM",
                version: 1,
                flags: 0,
                strings_offset: 0,
                parts_offset: 0,
                metadata_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            parts: Vec::new(),
            metadata: AssemblyMetadata::default(),
        };
        file.metadata.category = category as u32;
        file.metadata.name_id = file.add_string(name);
        file
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        // ДОБАВЛЕНО: переиспользуем уже добавленную строку вместо
        // дублирования — сборка с сотнями болтов реалистично повторяет
        // одни и те же имена/пути инструментов много раз ("wrench_10"
        // встретится у десятков креплений одного двигателя).
        if let Some(pos) = self.strings.iter().position(|existing| existing == s) {
            return pos as u32;
        }
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }

    pub fn get_string(&self, id: u32) -> Option<&str> {
        if id == NONE_ID { None } else { self.strings.get(id as usize).map(|s| s.as_str()) }
    }

    /// Добавляет корневую деталь (та, что "несёт" всю сборку — кузов,
    /// блок цилиндров). Ровно один раз на файл — вызывающая сторона сама
    /// следит за этим (см. `debug_assert!` ниже); движок при спавне
    /// (`engine/assembly.rs`) находит корень по `parent_index == -1` и
    /// упадёт в `debug_assert` там, если корней окажется не ровно один.
    pub fn add_root_part(&mut self, part: PartRecord) -> usize {
        debug_assert!(part.parent_index == -1, "корень должен иметь parent_index = -1");
        debug_assert!(
            !self.parts.iter().any(|p| p.parent_index == -1),
            "в сборке уже есть корневая деталь — вторая создаст неоднозначное дерево"
        );
        self.parts.push(part);
        self.parts.len() - 1
    }

    /// Добавляет деталь, скреплённую с уже существующей `parent_index`
    /// (индекс, возвращённый предыдущим `add_root_part`/`add_child_part`
    /// на ЭТОМ ЖЕ файле).
    pub fn add_child_part(&mut self, parent_index: usize, mut part: PartRecord) -> usize {
        part.parent_index = parent_index as i32;
        self.parts.push(part);
        self.parts.len() - 1
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            let bytes = s.as_bytes();
            strings_data.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            strings_data.extend_from_slice(bytes);
        }

        let mut parts_data = Vec::new();
        parts_data.extend_from_slice(&(self.parts.len() as u32).to_le_bytes());
        for part in &self.parts {
            parts_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(part as *const PartRecord as *const u8, std::mem::size_of::<PartRecord>())
            });
        }

        let metadata_data = unsafe {
            std::slice::from_raw_parts(&self.metadata as *const AssemblyMetadata as *const u8, std::mem::size_of::<AssemblyMetadata>())
        };

        let header_size = std::mem::size_of::<AlasmHeader>() as u64;
        let strings_offset = header_size;
        let parts_offset = strings_offset + strings_data.len() as u64;
        let metadata_offset = parts_offset + parts_data.len() as u64;

        let header = AlasmHeader {
            magic: self.header.magic,
            version: self.header.version,
            flags: self.header.flags,
            strings_offset,
            parts_offset,
            metadata_offset,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlasmHeader as *const u8, std::mem::size_of::<AlasmHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&parts_data)?;
        file.write_all(metadata_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut header_bytes = vec![0u8; std::mem::size_of::<AlasmHeader>()];
        file.read_exact(&mut header_bytes)?;
        let header: AlasmHeader = unsafe { std::ptr::read(header_bytes.as_ptr() as *const AlasmHeader) };

        if &header.magic != b"ALKALASM" {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "Invalid .alasm file"));
        }

        let mut asm = AlasmFile {
            header,
            strings: Vec::new(),
            parts: Vec::new(),
            metadata: AssemblyMetadata::default(),
        };

        file.seek(SeekFrom::Start(asm.header.strings_offset))?;
        let mut count_bytes = [0u8; 4];
        file.read_exact(&mut count_bytes)?;
        let str_count = u32::from_le_bytes(count_bytes);
        for _ in 0..str_count {
            let mut len_bytes = [0u8; 4];
            file.read_exact(&mut len_bytes)?;
            let len = u32::from_le_bytes(len_bytes) as usize;
            let mut bytes = vec![0u8; len];
            file.read_exact(&mut bytes)?;
            asm.strings.push(String::from_utf8_lossy(&bytes).to_string());
        }

        file.seek(SeekFrom::Start(asm.header.parts_offset))?;
        let mut parts_count_bytes = [0u8; 4];
        file.read_exact(&mut parts_count_bytes)?;
        let parts_count = u32::from_le_bytes(parts_count_bytes) as usize;
        asm.parts.reserve(parts_count);
        for _ in 0..parts_count {
            let mut part: PartRecord = unsafe { std::mem::zeroed() };
            file.read_exact(unsafe {
                std::slice::from_raw_parts_mut(&mut part as *mut PartRecord as *mut u8, std::mem::size_of::<PartRecord>())
            })?;
            asm.parts.push(part);
        }

        file.seek(SeekFrom::Start(asm.header.metadata_offset))?;
        let mut metadata: AssemblyMetadata = unsafe { std::mem::zeroed() };
        file.read_exact(unsafe {
            std::slice::from_raw_parts_mut(&mut metadata as *mut AssemblyMetadata as *mut u8, std::mem::size_of::<AssemblyMetadata>())
        })?;
        asm.metadata = metadata;

        Ok(asm)
    }

    /// Индекс корневой детали (`parent_index == -1`) — `None` для пустой
    /// или повреждённой сборки (не должно происходить для файла,
    /// созданного через `add_root_part`, но `load()` не может доверять
    /// содержимому произвольного файла с диска).
    pub fn root_index(&self) -> Option<usize> {
        self.parts.iter().position(|p| p.parent_index == -1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_preserves_tree_and_strings() {
        let mut asm = AlasmFile::new("ВАЗ-2106 двигатель 1.6", AssemblyCategory::Engine);

        let block_mesh = asm.add_string("meshes/engine_block.altex");
        let block_name = asm.add_string("Блок цилиндров");
        let block = asm.add_root_part(PartRecord {
            mass: 45.0,
            mesh_path_id: block_mesh,
            name_id: block_name,
            ..Default::default()
        });

        let head_mesh = asm.add_string("meshes/engine_head.altex");
        let head_name = asm.add_string("Головка блока цилиндров");
        let head_tool = asm.add_string("wrench_13");
        let head = asm.add_child_part(block, PartRecord {
            joint_type: 2, // FIXED
            mass: 12.0,
            break_impulse_linear: 500.0,
            mesh_path_id: head_mesh,
            name_id: head_name,
            required_tool_id: head_tool,
            ..Default::default()
        });

        let cover_mesh = asm.add_string("meshes/valve_cover.altex");
        let cover_name = asm.add_string("Крышка клапанов");
        let cover_tool = asm.add_string("wrench_10");
        let _valve_cover = asm.add_child_part(head, PartRecord {
            joint_type: 2,
            mass: 1.5,
            break_impulse_linear: 40.0,
            mesh_path_id: cover_mesh,
            name_id: cover_name,
            required_tool_id: cover_tool,
            ..Default::default()
        });

        let path = std::env::temp_dir().join("alasm_round_trip_test.alasm");
        let path_str = path.to_string_lossy().to_string();
        asm.save(&path_str).expect("save");
        let loaded = AlasmFile::load(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.parts.len(), 3);
        assert_eq!(loaded.metadata.category(), AssemblyCategory::Engine);
        assert_eq!(loaded.get_string(loaded.metadata.name_id), Some("ВАЗ-2106 двигатель 1.6"));
        assert_eq!(loaded.root_index(), Some(0));
        assert_eq!(loaded.parts[1].parent_index, 0);
        assert_eq!(loaded.parts[2].parent_index, 1);
        assert_eq!(loaded.get_string(loaded.parts[2].name_id), Some("Крышка клапанов"));
        assert_eq!(loaded.get_string(loaded.parts[2].required_tool_id), Some("wrench_10"));
        assert!((loaded.parts[1].break_impulse_linear - 500.0).abs() < 0.001);
    }
}
