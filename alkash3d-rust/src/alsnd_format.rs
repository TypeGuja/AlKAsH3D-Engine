// alsnd_format.rs - Spatial Sound System

use std::io::{Read, Write};

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AlsndHeader {
    pub magic: [u8; 8],           // "ALKALSND"
    pub version: u32,
    pub audio_engine: u32,        // 0=XAudio2, 1=WASAPI, 2=OpenAL, 3=Custom
    pub channels: u32,            // 2, 5.1, 7.1
    pub sample_rate: u32,         // 44100, 48000, 96000
    pub bits_per_sample: u32,     // 16, 24, 32
    pub sound_count: u32,
    pub sound_bank_count: u32,
    pub max_concurrent_sounds: u32, // 128
    pub string_table_offset: u64,
    pub sound_table_offset: u64,
    pub bank_table_offset: u64,
    pub preset_table_offset: u64,
    pub reverb_zones_offset: u64,
    pub occlusion_data_offset: u64,
    pub created_at: u64,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SoundDescriptor {
    pub name_id: u32,
    pub format: u32,              // 0=WAV, 1=OGG, 2=MP3, 3=FLAC, 4=OPUS
    pub category: u32,            // 0=SFX, 1=Music, 2=Ambient, 3=Voice, 4=UI
    pub data_offset: u64,
    pub size_compressed: u64,
    pub size_uncompressed: u64,
    pub duration_ms: u32,
    pub loop_start_ms: u32,
    pub loop_end_ms: u32,
    pub default_volume: f32,
    pub default_pitch: f32,
    pub priority: u32,            // 0 (низкий) - 255 (критический)
    pub max_instances: u32,       // Максимум одновременных проигрываний
    pub spatial_blend: f32,       // 0.0 (2D) - 1.0 (полностью 3D)
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SoundBank {
    pub name_id: u32,
    pub sounds_start: u32,
    pub sounds_count: u32,
    pub preload_all: u32,
    pub keep_in_memory: u32,
    pub memory_budget_mb: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct SoundPreset {
    pub name_id: u32,
    pub sounds: [u32; 8],         // До 8 звуков на пресет
    pub weights: [f32; 8],        // Веса для случайного выбора
    pub sound_count: u32,
    pub randomize_pitch: [f32; 2], // min, max
    pub randomize_volume: [f32; 2],
    pub attenuation_model: u32,   // 0=linear, 1=log, 2=custom
    pub min_distance: f32,
    pub max_distance: f32,
    pub doppler_factor: f32,
    pub cone_inner_angle: f32,
    pub cone_outer_angle: f32,
    pub cone_outer_gain: f32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct ReverbZone {
    pub position: [f32; 3],
    pub radius: f32,
    pub room_size: f32,
    pub damping: f32,
    pub wet_level: f32,
    pub dry_level: f32,
    pub width: f32,
    pub preset: u32,              // 0=generic, 1=hall, 2=room, 3=chamber, 4=custom
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AudioOcclusion {
    pub source_position: [f32; 3],
    pub listener_position: [f32; 3],
    pub direct_occlusion: f32,    // 0.0 (полностью перекрыто) - 1.0 (открыто)
    pub reverb_occlusion: f32,
    pub material_id: u32,         // Материал препятствия для фильтрации
    pub frequency_attenuation: [f32; 8], // Аттенюация по октавам
}

pub struct AlsndFile {
    pub header: AlsndHeader,
    pub strings: Vec<String>,
    pub sounds: Vec<SoundDescriptor>,
    pub banks: Vec<SoundBank>,
    pub presets: Vec<SoundPreset>,
    pub reverb_zones: Vec<ReverbZone>,
}

impl AlsndFile {
    pub fn new(channels: u32, sample_rate: u32) -> Self {
        Self {
            header: AlsndHeader {
                magic: *b"ALKALSND",
                version: 1,
                audio_engine: 0,      // XAudio2 по умолчанию
                channels,
                sample_rate,
                bits_per_sample: 16,
                sound_count: 0,
                sound_bank_count: 0,
                max_concurrent_sounds: 128,
                string_table_offset: 0,
                sound_table_offset: 0,
                bank_table_offset: 0,
                preset_table_offset: 0,
                reverb_zones_offset: 0,
                occlusion_data_offset: 0,
                created_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap().as_secs(),
            },
            strings: Vec::new(),
            sounds: Vec::new(),
            banks: Vec::new(),
            presets: Vec::new(),
            reverb_zones: Vec::new(),
        }
    }

    pub fn create_city_ambient() -> Self {
        let mut snd = AlsndFile::new(2, 48000);

        // Сначала получаем ID строки
        let name_id = snd.add_string("City_Ambient_Day");

        // Потом используем его
        snd.presets.push(SoundPreset {
            name_id,
            sounds: [0, 1, 2, 3, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF, 0xFFFFFFFF],
            weights: [0.3, 0.3, 0.2, 0.2, 0.0, 0.0, 0.0, 0.0],
            sound_count: 4,
            randomize_pitch: [0.95, 1.05],
            randomize_volume: [0.8, 1.0],
            attenuation_model: 1,
            min_distance: 10.0,
            max_distance: 200.0,
            doppler_factor: 0.5,
            cone_inner_angle: 360.0,
            cone_outer_angle: 360.0,
            cone_outer_gain: 0.0,
        });

        snd
    }

    // ИСПРАВЛЕНО (звуковая подсистема — save/load): раньше `add_string` не
    // проверял, есть ли уже такая строка в таблице, и всегда добавлял новую
    // — единственный формат движка (.alfar/.altex/.alcar/.alworld все
    // дедуплицируют) с таким расхождением. При типичном использовании
    // (`create_city_ambient` вызывает `add_string("City_Ambient_Day")`
    // один раз, так что раньше это было незаметно) разницы не было, но при
    // добавлении множества звуков с повторяющимися именами (например
    // несколько пресетов, ссылающихся на одно и то же имя категории)
    // раздувало бы строковую таблицу дубликатами. Теперь ведёт себя как
    // остальные форматы.
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

    // =========================================================================
    // ДОБАВЛЕНО (звуковая подсистема — save()/load() для .alsnd): раньше
    // ЕДИНСТВЕННЫЙ формат движка без сериализации на диск вообще (в отличие
    // от .alfar/.altex/.alcar/.alworld — у всех есть работающий save/load).
    // Формат совпадает по духу с `.alworld` (см. AlworldFile::save/load в
    // alworld_format.rs): header (с offset'ами на каждую таблицу) -> string
    // table (count + [len(u32)+bytes]) -> sounds -> banks -> presets ->
    // reverb_zones, каждая таблица — count(u32) + POD-массив своей
    // структуры. AudioOcclusion сознательно НЕ сериализуется как часть
    // файла — это не статические данные карты звука, а результат расчёта
    // окклюзии между конкретным источником и слушателем в ТЕКУЩЕМ кадре
    // (см. поля source_position/listener_position — они бессмысленны без
    // текущих мировых координат обоих), т.е. чисто рантайм-величина,
    // вычисляемая заново каждый кадр аудио-плейбек движком (см. audio.rs),
    // а не то, что имеет смысл сохранять на диск — как и `state` у
    // ChunkDescriptor в .alworld или как отсутствие сохранения текущих
    // AABB в .altex.
    // =========================================================================
    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let mut file = std::fs::File::create(path)?;

        let mut strings_data = Vec::new();
        strings_data.extend_from_slice(&(self.strings.len() as u32).to_le_bytes());
        for s in &self.strings {
            strings_data.extend_from_slice(&(s.len() as u32).to_le_bytes());
            strings_data.extend_from_slice(s.as_bytes());
        }

        let mut sounds_data = Vec::new();
        sounds_data.extend_from_slice(&(self.sounds.len() as u32).to_le_bytes());
        for sound in &self.sounds {
            sounds_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(sound as *const SoundDescriptor as *const u8, std::mem::size_of::<SoundDescriptor>())
            });
        }

        let mut banks_data = Vec::new();
        banks_data.extend_from_slice(&(self.banks.len() as u32).to_le_bytes());
        for bank in &self.banks {
            banks_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(bank as *const SoundBank as *const u8, std::mem::size_of::<SoundBank>())
            });
        }

        let mut presets_data = Vec::new();
        presets_data.extend_from_slice(&(self.presets.len() as u32).to_le_bytes());
        for preset in &self.presets {
            presets_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(preset as *const SoundPreset as *const u8, std::mem::size_of::<SoundPreset>())
            });
        }

        let mut reverb_data = Vec::new();
        reverb_data.extend_from_slice(&(self.reverb_zones.len() as u32).to_le_bytes());
        for zone in &self.reverb_zones {
            reverb_data.extend_from_slice(unsafe {
                std::slice::from_raw_parts(zone as *const ReverbZone as *const u8, std::mem::size_of::<ReverbZone>())
            });
        }

        let header_size = std::mem::size_of::<AlsndHeader>() as u64;
        let string_table_offset = header_size;
        let sound_table_offset = string_table_offset + strings_data.len() as u64;
        let bank_table_offset = sound_table_offset + sounds_data.len() as u64;
        let preset_table_offset = bank_table_offset + banks_data.len() as u64;
        let reverb_zones_offset = preset_table_offset + presets_data.len() as u64;
        // occlusion_data_offset намеренно не указывает ни на какой блок в
        // файле (окклюзия не сериализуется, см. комментарий выше) —
        // оставлен 0 как явный "не используется", тот же приём, что и у
        // некоторых offset-полей в .alworld/.altex, когда соответствующий
        // блок в конкретном файле отсутствует.
        let occlusion_data_offset = 0u64;

        let header = AlsndHeader {
            magic: self.header.magic,
            version: self.header.version,
            audio_engine: self.header.audio_engine,
            channels: self.header.channels,
            sample_rate: self.header.sample_rate,
            bits_per_sample: self.header.bits_per_sample,
            sound_count: self.sounds.len() as u32,
            sound_bank_count: self.banks.len() as u32,
            max_concurrent_sounds: self.header.max_concurrent_sounds,
            string_table_offset,
            sound_table_offset,
            bank_table_offset,
            preset_table_offset,
            reverb_zones_offset,
            occlusion_data_offset,
            created_at: self.header.created_at,
        };

        file.write_all(unsafe {
            std::slice::from_raw_parts(&header as *const AlsndHeader as *const u8, std::mem::size_of::<AlsndHeader>())
        })?;
        file.write_all(&strings_data)?;
        file.write_all(&sounds_data)?;
        file.write_all(&banks_data)?;
        file.write_all(&presets_data)?;
        file.write_all(&reverb_data)?;

        Ok(())
    }

    pub fn load(path: &str) -> std::io::Result<Self> {
        let mut file = std::fs::File::open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;

        let header_size = std::mem::size_of::<AlsndHeader>();
        if buf.len() < header_size {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alsnd: файл короче заголовка AlsndHeader",
            ));
        }

        // SAFETY: AlsndHeader — #[repr(C)], POD (только числа и [u8;8]),
        // длина буфера уже проверена выше.
        let header: AlsndHeader = unsafe {
            std::ptr::read_unaligned(buf.as_ptr() as *const AlsndHeader)
        };

        if &header.magic != b"ALKALSND" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alsnd: неверная сигнатура {:?}, ожидалось ALKALSND", header.magic),
            ));
        }
        if header.version != 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alsnd: неподдерживаемая версия формата {}", header.version),
            ));
        }

        let read_at = |offset: u64, size: usize, what: &str| -> std::io::Result<&[u8]> {
            let start = offset as usize;
            let end = start.checked_add(size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alsnd: переполнение при вычислении конца блока {}", what),
            ))?;
            buf.get(start..end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("alsnd: блок {} выходит за пределы файла (offset={}, size={}, file_len={})", what, offset, size, buf.len()),
            ))
        };

        // Строковая таблица: count(u32) + N раз [len(u32) + байты строки].
        let strings_start = header.string_table_offset as usize;
        if strings_start > buf.len() || strings_start + 4 > buf.len() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "alsnd: string_table_offset выходит за пределы файла",
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

        // Небольшой локальный хелпер, читающий "count(u32) + POD-массив T"
        // блок по заданному offset'у — используется для sounds/banks/
        // presets/reverb_zones ниже, все четыре имеют абсолютно одинаковую
        // структуру блока, отличается только тип T.
        fn read_table<T: Copy>(buf: &[u8], offset: u64, what: &str) -> std::io::Result<Vec<T>> {
            let start = offset as usize;
            if start + 4 > buf.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    format!("alsnd: {}_offset выходит за пределы файла", what),
                ));
            }
            let count = u32::from_le_bytes(buf[start..start + 4].try_into().unwrap()) as usize;
            let item_size = std::mem::size_of::<T>();
            let data_start = start + 4;
            let data_end = data_start.checked_add(count * item_size).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("alsnd: переполнение при вычислении размера таблицы {}", what),
            ))?;
            let bytes = buf.get(data_start..data_end).ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!("alsnd: таблица {} выходит за пределы файла (offset={}, count={}, file_len={})", what, offset, count, buf.len()),
            ))?;
            let mut items = Vec::with_capacity(count);
            for i in 0..count {
                let s = i * item_size;
                let item: T = unsafe { std::ptr::read_unaligned(bytes[s..s + item_size].as_ptr() as *const T) };
                items.push(item);
            }
            Ok(items)
        }

        let sounds: Vec<SoundDescriptor> = read_table(&buf, header.sound_table_offset, "sound_table")?;
        let banks: Vec<SoundBank> = read_table(&buf, header.bank_table_offset, "bank_table")?;
        let presets: Vec<SoundPreset> = read_table(&buf, header.preset_table_offset, "preset_table")?;
        let reverb_zones: Vec<ReverbZone> = read_table(&buf, header.reverb_zones_offset, "reverb_zones")?;

        Ok(Self {
            header,
            strings,
            sounds,
            banks,
            presets,
            reverb_zones,
        })
    }
}

impl Default for AlsndFile {
    fn default() -> Self {
        Self::new(2, 48000)
    }
}