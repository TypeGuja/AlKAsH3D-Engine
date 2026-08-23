// alfar_format.rs - Light Archive

use std::io::{Read, Write, Seek, SeekFrom};

#[repr(C)]
pub struct AlfarHeader {
    pub magic: [u8; 8],
    pub version: u32,
    pub flags: u32,
    pub ambient_offset: u64,
    pub global_settings_offset: u64,
    pub individual_lights_offset: u64,
    pub light_groups_offset: u64,
    pub animation_offset: u64,
    pub total_lights: u32,
    pub light_groups_count: u32,
    pub created_at: u64,
}

// ДОБАВЛЕНО (Фаза 1 плана по реализму/фонарям): Clone/Copy — теперь эти
// структуры хранятся напрямую в `AlkashEngine` (см.
// `AlkashEngine::light_ambient`/`light_global_settings`,
// `load_lights_from_alfar` в engine/mod.rs) и должны свободно читаться
// каждый кадр без потребления/перемещения владения; обе — плоские POD
// (только f32/u32/[f32;N]), так что Copy ничего не ломает.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AmbientLight {
    pub intensity: f32,
    pub color: [f32; 3],
    pub skybox_intensity: f32,
    pub use_skybox: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct GlobalLightSettings {
    pub master_intensity: f32,
    pub shadow_quality: u32,
    pub shadow_distance: f32,
    pub cascade_count: u32,
    pub volumetric_enabled: u32,
    pub volumetric_quality: u32,
    pub bloom_intensity: f32,
    pub exposure: f32,
    pub gamma: f32,
}

#[repr(u32)]
pub enum LightType {
    Point = 0,
    Spot = 1,
    Directional = 2,
    Area = 3,
}

#[repr(u32)]
pub enum LightFalloff {
    Linear = 0,
    Quadratic = 1,
    Cubic = 2,
    Custom = 3,
}

#[repr(C)]
pub struct IndividualLight {
    pub id: u32,
    pub name_id: u32,
    pub light_type: u32,
    pub position: [f32; 3],
    pub direction: [f32; 3],
    pub up: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
    pub falloff_type: u32,
    pub falloff_custom: f32,
    pub spot_inner_angle: f32,
    pub spot_outer_angle: f32,
    pub casts_shadows: u32,
    pub shadow_bias: f32,
    pub shadow_resolution: u32,
    pub flicker_enabled: u32,
    pub flicker_speed: f32,
    pub flicker_intensity: f32,
    pub enabled: u32,
    pub active_from: f32,
    pub active_to: f32,
    pub has_physics: u32,
    pub breakable: u32,
    pub health: f32,
    pub custom_data_offset: u64,
}

#[repr(C)]
pub struct LightGroup {
    pub group_id: u32,
    pub name_id: u32,
    /// СЫРОЙ указатель для совместимости с C ABI. Сам он НЕ владеет данными —
    /// фактический буфер должен жить где-то ещё как минимум столько же,
    /// сколько живёт этот LightGroup. См. `AlfarFile::group_light_ids`
    /// и комментарий в `add_light_group`.
    pub light_ids: *mut u32,
    pub light_count: u32,
    pub master_intensity: f32,
    pub color_tint: [f32; 3],
    pub sync_flicker: u32,
    pub group_enabled: u32,
    pub active_from: f32,
    pub active_to: f32,
}

#[repr(C)]
pub struct LightAnimation {
    pub light_id: u32,
    pub keyframe_count: u32,
    pub keyframe_offset: u64,
}

#[repr(C)]
pub struct Keyframe {
    pub time: f32,
    pub intensity: f32,
    pub color: [f32; 3],
    pub position: [f32; 3],
    pub interpolation: u32,
}

pub struct AlfarFile {
    pub header: AlfarHeader,
    pub strings: Vec<String>,
    pub ambient: AmbientLight,
    pub global_settings: GlobalLightSettings,
    pub lights: Vec<IndividualLight>,
    pub light_groups: Vec<LightGroup>,
    pub animations: Vec<LightAnimation>,
    pub keyframes: Vec<Keyframe>,
    pub custom_data: Vec<u8>,
    /// ИСПРАВЛЕНО (было use-after-free): раньше `light_ids` в `LightGroup`
    /// указывал на буфер `Vec<u32>`, который создавался локально внутри
    /// `add_light_group` и уничтожался в конце той же функции (через
    /// `for id in ids { ... }`, потребляющий `ids` по значению). Указатель
    /// в `LightGroup` мгновенно становился висячим сразу после возврата из
    /// функции. Теперь каждый буфер id-шников реально хранится здесь, в
    /// `AlfarFile`, и живёт как минимум столько же, сколько сам файл.
    ///
    /// Важно: перемещение `AlfarFile` (например, возврат по значению) не
    /// инвалидирует эти указатели — двигается только внешний `Vec<Vec<u32>>`
    /// (его собственные стек-поля: указатель/длина/capacity), а сами
    /// внутренние буферы `Vec<u32>` живут в куче по стабильным адресам.
    /// НЕ удаляй и не изменяй элементы `group_light_ids` после того, как на
    /// них уже завели `LightGroup` — это единственный инвариант, который
    /// нужно соблюдать вручную.
    group_light_ids: Vec<Vec<u32>>,
}

impl AlfarFile {
    pub fn new() -> Self {
        Self {
            header: AlfarHeader {
                magic: *b"ALKALFAR",
                version: 1,
                flags: 0,
                ambient_offset: 0,
                global_settings_offset: 0,
                individual_lights_offset: 0,
                light_groups_offset: 0,
                animation_offset: 0,
                total_lights: 0,
                light_groups_count: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            ambient: AmbientLight {
                intensity: 0.2,
                color: [0.2, 0.25, 0.3],
                skybox_intensity: 0.5,
                use_skybox: 1,
            },
            global_settings: GlobalLightSettings {
                master_intensity: 1.0,
                shadow_quality: 2,
                shadow_distance: 100.0,
                cascade_count: 4,
                volumetric_enabled: 1,
                volumetric_quality: 1,
                bloom_intensity: 0.3,
                exposure: 1.0,
                gamma: 2.2,
            },
            lights: Vec::new(),
            light_groups: Vec::new(),
            animations: Vec::new(),
            keyframes: Vec::new(),
            custom_data: Vec::new(),
            group_light_ids: Vec::new(),
        }
    }

    pub fn add_string(&mut self, s: &str) -> u32 {
        let id = self.strings.len() as u32;
        self.strings.push(s.to_string());
        id
    }

    pub fn add_light(&mut self, mut light: IndividualLight, name: &str) -> u32 {
        let light_id = self.lights.len() as u32;
        let name_id = self.add_string(name);
        light.id = light_id;
        light.name_id = name_id;
        self.lights.push(light);
        self.header.total_lights += 1;
        light_id
    }

    pub fn add_light_group(&mut self, name: &str, light_ids: &[u32]) -> u32 {
        let group_id = self.light_groups.len() as u32;
        let name_id = self.add_string(name);

        // ИСПРАВЛЕНО: владеющая копия id-шников теперь хранится в
        // `self.group_light_ids`, а не только локально в этой функции —
        // поэтому указатель `light_ids` в LightGroup остаётся валидным
        // всё время жизни AlfarFile (подробности см. в комментарии у поля
        // `group_light_ids`).
        let mut ids = Vec::from(light_ids);
        let ptr = ids.as_mut_ptr();
        let count = ids.len() as u32;
        self.group_light_ids.push(ids);

        self.light_groups.push(LightGroup {
            group_id,
            name_id,
            light_ids: ptr,
            light_count: count,
            master_intensity: 1.0,
            color_tint: [1.0, 1.0, 1.0],
            sync_flicker: 0,
            group_enabled: 1,
            active_from: 0.0,
            active_to: 24.0,
        });

        // Дублируем id-шники в custom_data (для будущей сериализации групп;
        // на сегодня save() групп ещё не пишет, см. groups_data ниже).
        // Итерируемся по ссылке на владеющую копию, а не потребляем её.
        for id in self.group_light_ids.last().unwrap() {
            self.custom_data.extend_from_slice(&id.to_le_bytes());
        }

        self.header.light_groups_count += 1;
        group_id
    }

    pub fn create_night_city() -> Self {
        let mut alfar = AlfarFile::new();

        alfar.ambient = AmbientLight {
            intensity: 0.05,
            color: [0.05, 0.05, 0.1],
            skybox_intensity: 0.1,
            use_skybox: 1,
        };

        alfar.global_settings = GlobalLightSettings {
            master_intensity: 1.0,
            shadow_quality: 2,
            shadow_distance: 50.0,
            cascade_count: 4,
            volumetric_enabled: 1,
            volumetric_quality: 2,
            bloom_intensity: 0.8,
            exposure: 0.8,
            gamma: 2.2,
        };

        for i in 0..20 {
            let x = (i as f32 - 10.0) * 5.0;
            let light = IndividualLight {
                id: 0,
                name_id: 0,
                light_type: LightType::Point as u32,
                position: [x, 3.0, 0.0],
                direction: [0.0, -1.0, 0.0],
                up: [0.0, 1.0, 0.0],
                color: [1.0, 0.85, 0.6],
                intensity: 2.0,
                // ИЗМЕНЕНО (по просьбе): дальность действия уличных фонарей
                // увеличена с 15.0 до 100.0 (радиус физического влияния света на
                // освещаемую им поверхность, windowFalloff в
                // ComputePointLightContribution, engine/mod.rs) — фонари
                // светят значительно дальше вдоль улицы. См. также правку
                // MAX_CELLS_PER_LIGHT в alkash3d-FirstFires/src/lib.rs:
                // при таком range сфера действия фонаря пересекает намного
                // больше ячеек пространственной сетки каллинга, чем при
                // старом значении — без соответствующего поднятия лимита
                // часть ячеек не получила бы регистрацию света, что
                // выглядело бы как резкие обрывы освещённости на границах
                // ячеек (тот же класс бага, что уже был исправлен раньше).
                range: 100.0,
                falloff_type: LightFalloff::Quadratic as u32,
                falloff_custom: 0.0,
                spot_inner_angle: 0.0,
                spot_outer_angle: 0.0,
                casts_shadows: 1,
                shadow_bias: 0.005,
                shadow_resolution: 1024,
                flicker_enabled: 1,
                flicker_speed: 2.0,
                flicker_intensity: 0.15,
                enabled: 1,
                active_from: 18.0,
                active_to: 6.0,
                has_physics: 0,
                breakable: 0,
                health: 0.0,
                custom_data_offset: 0,
            };
            alfar.add_light(light, &format!("StreetLight_{}", i));
        }

        alfar
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let ambient_data = unsafe {
            std::slice::from_raw_parts(&self.ambient as *const AmbientLight as *const u8, std::mem::size_of::<AmbientLight>())
        };
        let global_data = unsafe {
            std::slice::from_raw_parts(&self.global_settings as *const GlobalLightSettings as *const u8, std::mem::size_of::<GlobalLightSettings>())
        };

        let mut lights_data = Vec::new();
        lights_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            lights_data.extend_from_slice(s.as_bytes());
            lights_data.push(0);
        }
        for light in &self.lights {
            lights_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(light as *const IndividualLight as *const u8, std::mem::size_of::<IndividualLight>())
            });
        }

        let groups_data = Vec::new();
        let anim_data = Vec::new();

        let header_size = std::mem::size_of::<AlfarHeader>() as u64;
        let ambient_offset = header_size;
        let global_offset = ambient_offset + ambient_data.len() as u64;
        let lights_offset = global_offset + global_data.len() as u64;
        let groups_offset = lights_offset + lights_data.len() as u64;
        let anim_offset = groups_offset + groups_data.len() as u64;

        let header = AlfarHeader {
            magic: self.header.magic,
            version: self.header.version,
            flags: self.header.flags,
            ambient_offset,
            global_settings_offset: global_offset,
            individual_lights_offset: lights_offset,
            light_groups_offset: groups_offset,
            animation_offset: anim_offset,
            total_lights: self.lights.len() as u32,
            light_groups_count: self.light_groups.len() as u32,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlfarHeader as *const u8, std::mem::size_of::<AlfarHeader>())
        })?;
        file.write_all(ambient_data)?;
        file.write_all(global_data)?;
        file.write_all(&lights_data)?;
        file.write_all(&groups_data)?;
        file.write_all(&anim_data)?;

        Ok(())
    }

    pub fn get_light(&self, id: u32) -> Option<&IndividualLight> {
        self.lights.get(id as usize)
    }

    pub fn get_group(&self, id: u32) -> Option<&LightGroup> {
        self.light_groups.get(id as usize)
    }

    // ================================================================
    // ДОБАВЛЕНО (Фаза 1 плана по реализму/фонарям): чтение .alfar с диска.
    //
    // До этого момента формат использовался ТОЛЬКО на запись (`save()`) —
    // ничего в движке не могло прочитать .alfar обратно, то есть вся
    // богатая модель освещения (тени/мерцание/день-ночь/volumetrics),
    // которую формат уже поддерживал, была недостижима в рантайме.
    //
    // `load()` — зеркало `save()`, читает ровно те же блоки в том же
    // порядке и с теми же смещениями, которые вычисляет `save()`:
    // header -> ambient -> global_settings -> lights_data (count строк +
    // сами строки, null-terminated + все IndividualLight подряд).
    //
    // ВАЖНО: `save()` на сегодня НЕ сериализует реальные данные групп и
    // анимаций (`groups_data`/`anim_data` в save() всегда пустые Vec,
    // несмотря на то что `light_groups_count` в заголовке пишется) —
    // поэтому `load()` тоже не пытается их читать, только считает
    // `light_groups_count`/`total_lights` как метаданные для валидации.
    // Если/когда `save()` научится реально писать группы и анимации,
    // `load()` нужно будет расширить симметрично — до тех пор попытка
    // прочитать несуществующие байты групп была бы либо чтением мусора,
    // либо чтением байт следующего блока не по адресу.
    // ================================================================
    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let header_size = std::mem::size_of::<AlfarHeader>();
        if buf.len() < header_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alfar: файл короче заголовка AlfarHeader",
            ));
        }

        // SAFETY: AlfarHeader — #[repr(C)], POD (только числа и [u8;8]),
        // мы только что проверили, что в буфере достаточно байт.
        let header: AlfarHeader = unsafe {
            std::ptr::read_unaligned(buf.as_ptr() as *const AlfarHeader)
        };

        if &header.magic != b"ALKALFAR" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alfar: неверная сигнатура {:?}, ожидалось ALKALFAR", header.magic),
            ));
        }
        if header.version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alfar: неподдерживаемая версия формата {}", header.version),
            ));
        }

        let read_at = |offset: u64, size: usize, what: &str| -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.checked_add(size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alfar: переполнение при вычислении конца блока {}", what),
            ))?;
            buf.get(start..end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("alfar: блок {} выходит за пределы файла (offset={}, size={}, file_len={})", what, offset, size, buf.len()),
            ))
        };

        let ambient_bytes = read_at(header.ambient_offset, std::mem::size_of::<AmbientLight>(), "ambient")?;
        let ambient: AmbientLight = unsafe { std::ptr::read_unaligned(ambient_bytes.as_ptr() as *const AmbientLight) };

        let global_bytes = read_at(header.global_settings_offset, std::mem::size_of::<GlobalLightSettings>(), "global_settings")?;
        let global_settings: GlobalLightSettings = unsafe { std::ptr::read_unaligned(global_bytes.as_ptr() as *const GlobalLightSettings) };

        // Блок lights_data начинается с individual_lights_offset и тянется
        // до конца файла (либо до groups_offset, если он больше — на
        // сегодня groups_data всегда пуст, так что оба варианта совпадают;
        // берём min(groups_offset, file_len), чтобы не читать за пределы
        // файла даже если когда-нибудь появятся непустые группы старой
        // сериализации без апдейта load()).
        let lights_start = header.individual_lights_offset as usize;
        if lights_start > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alfar: individual_lights_offset выходит за пределы файла",
            ));
        }
        let lights_end = (header.light_groups_offset as usize).min(buf.len()).max(lights_start);
        let lights_data = &buf[lights_start..lights_end];

        if lights_data.len() < 4 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alfar: блок lights_data короче счётчика строк",
            ));
        }

        let mut cursor = 0usize;
        let string_count = u32::from_le_bytes(lights_data[0..4].try_into().unwrap()) as usize;
        cursor += 4;

        let mut strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            let nul_pos = lights_data[cursor..].iter().position(|&b| b == 0).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "alfar: строка без завершающего нуля")
            })?;
            let s = String::from_utf8_lossy(&lights_data[cursor..cursor + nul_pos]).into_owned();
            strings.push(s);
            cursor += nul_pos + 1;
        }

        let light_size = std::mem::size_of::<IndividualLight>();
        let remaining = &lights_data[cursor..];
        if remaining.len() % light_size != 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "alfar: остаток блока источников света ({} байт) не кратен размеру IndividualLight ({} байт)",
                    remaining.len(), light_size
                ),
            ));
        }
        let light_count = remaining.len() / light_size;
        let mut lights = Vec::with_capacity(light_count);
        for i in 0..light_count {
            let start = i * light_size;
            let light: IndividualLight = unsafe {
                std::ptr::read_unaligned(remaining[start..start + light_size].as_ptr() as *const IndividualLight)
            };
            lights.push(light);
        }

        if lights.len() != header.total_lights as usize {
            eprintln!(
                "[ALFAR] ПРЕДУПРЕЖДЕНИЕ: заголовок обещает total_lights={}, реально прочитано {} — файл мог быть обрезан или повреждён",
                header.total_lights, lights.len()
            );
        }

        Ok(Self {
            header,
            strings,
            ambient,
            global_settings,
            lights,
            // Группы/анимации на диске сейчас не хранятся (см. комментарий
            // выше) — пустые, а не выдуманные из воздуха данные.
            light_groups: Vec::new(),
            animations: Vec::new(),
            keyframes: Vec::new(),
            custom_data: Vec::new(),
            group_light_ids: Vec::new(),
        })
    }
}