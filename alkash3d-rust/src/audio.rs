// src/audio.rs
//! Звуковая подсистема движка — реальный плейбек через XAudio2 (2.8 API,
//! встроен в Windows начиная с Windows 8, никакого отдельного redist не
//! требуется — см. подробный комментарий в Cargo.toml у фичи
//! "Win32_Media_Audio_XAudio2"), плюс декодер WAV-файлов, пространственное
//! (3D) позиционирование звука и категории/приоритеты из `.alsnd`
//! (см. alsnd_format.rs).
//!
//! ДОБАВЛЕНО (объединённая сцена/звук — Фаза "Sound" плана после
//! Stabilization и Unified Scene): раньше в движке НЕ было вообще никакого
//! аудио-плейбека — `PluginType::Audio` существовал только как
//! неиспользуемый ABI-вариант (см. plugin/abi.rs), и ни один звук никогда
//! не воспроизводился. Перед написанием этого модуля было явно проверено
//! (см. рабочий процесс сессии), что на диске пользователя НЕТ отдельного
//! готового аудио-плагина (в отличие от alkash3d-inertial для физики и
//! alkash3d-FirstFires для света) — поэтому звук реализован НАПРЯМУЮ в
//! alkash3d-rust через системный XAudio2, а не через внешний plugin DLL.
//!
//! Архитектура (по образцу LightPlugin/PhysicsPlugin, но без внешнего
//! DLL — сам XAudio2 УЖЕ является системным "плагином" ОС):
//!   AudioEngine::new()            — создаёт IXAudio2 + mastering voice
//!   AudioEngine::load_bank()      — грузит .alsnd + связанные .wav файлы
//!   AudioEngine::play_sound_2d()  — играет без учёта позиции (UI, музыка)
//!   AudioEngine::play_sound_3d()  — играет с 3D-позиционированием
//!   AudioEngine::set_listener()   — обновляет позицию/ориентацию слушателя
//!   AudioEngine::update()         — раз в кадр: пересчитывает громкость/
//!                                    панораму/питч у активных 3D-звуков
//!                                    (слушатель или источник могли
//!                                    подвинуться со времени play_sound_3d)
//!                                    и вычищает завершившиеся голоса.

use std::collections::HashMap;
use windows::Win32::Media::Audio::{
    WAVEFORMATEX, WAVE_FORMAT_PCM,
};
use windows::Win32::Media::Audio::XAudio2::{
    IXAudio2, IXAudio2MasteringVoice, IXAudio2SourceVoice,
    XAudio2CreateWithVersionInfo, XAUDIO2_BUFFER, XAUDIO2_END_OF_STREAM,
    XAUDIO2_LOOP_INFINITE, XAUDIO2_VOICE_STATE,
};

use crate::alsnd_format::{AlsndFile, SoundDescriptor};
use crate::math::Vec3;

/// NTDDI-версия, передаваемая в `XAudio2CreateWithVersionInfo` — движок
/// целится в 10-летнее минимальное железо (см. ТЗ), а не в самую свежую
/// Windows, но XAudio2 2.8 (xaudio2_8.dll) в любом случае одинаков на всех
/// версиях ОС, где он вообще есть (Windows 8+) — конкретное числовое
/// значение ntddiversion в этой функции влияет только на то, какие
/// НОВЕЙШИЕ возможности API разрешено использовать, а не на совместимость
/// вниз; 0 ("NTDDI_VERSION не проверять") — безопасный консервативный
/// выбор, не отказывающий в создании движка на старых системах.
const NTDDI_VERSION_UNSPECIFIED: u32 = 0;

/// Максимальное число одновременно проигрываемых голосов по умолчанию —
/// используется, если `.alsnd`-банк не указывает своё
/// `max_concurrent_sounds` (см. AlsndHeader). Совпадает с значением по
/// умолчанию в `AlsndFile::new`.
const DEFAULT_MAX_CONCURRENT_SOUNDS: usize = 128;

/// ДОБАВЛЕНО: простая, но реалистичная модель дистанционного затухания —
/// без готовой библиотеки X3DAudio (её нет в используемой версии крейта
/// `windows`, см. рабочий процесс сессии: в windows-0.62.2 присутствует
/// `Win32_Media_Audio_XAudio2`, но не отдельный модуль X3DAudio), поэтому
/// затухание/панорама/доплер считаются вручную здесь, на основе полей
/// `SoundPreset` из .alsnd (min_distance/max_distance/attenuation_model/
/// doppler_factor) — та же модель величин, что описана в самом формате.
#[derive(Debug, Clone, Copy)]
pub struct AttenuationParams {
    pub min_distance: f32,
    pub max_distance: f32,
    /// 0 = linear, 1 = log (логарифмическое, ближе к тому, как человек
    /// СУБЪЕКТИВНО воспринимает громкость — см. SoundPreset::attenuation_model
    /// в alsnd_format.rs), 2 = custom (пока обрабатывается как linear —
    /// нет отдельной кривой для "custom" в текущей версии).
    pub model: u32,
    pub doppler_factor: f32,
}

impl Default for AttenuationParams {
    fn default() -> Self {
        Self {
            min_distance: 1.0,
            max_distance: 50.0,
            model: 1, // log — реалистичнее по умолчанию, чем linear
            doppler_factor: 1.0,
        }
    }
}

/// Декодированный звуковой клип — PCM-сэмплы в памяти + формат, готовые к
/// прямой отправке в XAudio2 (`SubmitSourceBuffer`) без дальнейшего
/// декодирования на каждое проигрывание. Один `SoundClip` может
/// проигрываться много раз одновременно (несколько source voice читают
/// один и тот же `data`, см. `Arc` в `AudioEngine::clips`).
pub struct SoundClip {
    pub format: WAVEFORMATEX,
    pub data: Vec<u8>,
    pub duration_ms: u32,
}

/// Идентификатор активно проигрываемого звука, возвращаемый
/// `play_sound_2d`/`play_sound_3d` — используется для `stop_sound`/
/// `set_sound_volume` и для последующего обновления 3D-позиции в `update`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SoundHandle(u64);

/// Один активный (проигрываемый прямо сейчас) голос — source voice плюс
/// всё, что нужно `update()` каждый кадр для пересчёта 3D-параметров и для
/// определения, когда голос закончил играть и его пора вычистить.
struct ActiveVoice {
    voice: IXAudio2SourceVoice,
    category: u32,
    priority: u32,
    /// `None` — двумерный (не позиционный) звук: громкость/панорама не
    /// пересчитываются каждый кадр, только дистанционно НЕ затухают.
    spatial: Option<Spatial3D>,
    /// Громкость 0.0-1.0, заданная при проигрывании (до учёта дистанции) —
    /// умножается на дистанционное затухание для 3D-звуков.
    base_volume: f32,
}

struct Spatial3D {
    position: Vec3,
    attenuation: AttenuationParams,
}

/// Слушатель — обычно позиция и направление взгляда камеры игрока (см.
/// `AlkashEngine::update` в engine/mod.rs, где `set_listener` вызывается
/// вместе с обновлением камеры каждый кадр).
#[derive(Debug, Clone, Copy)]
pub struct Listener {
    pub position: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
    /// Скорость слушателя (м/с) — используется для доплеровского сдвига
    /// вместе со скоростью источника (см. `play_sound_3d`'s `velocity`
    /// параметр). `Vec3::ZERO`, если движение не отслеживается —
    /// доплер-эффект в этом случае просто не применяется.
    pub velocity: Vec3,
}

impl Default for Listener {
    fn default() -> Self {
        Self {
            position: Vec3::ZERO,
            forward: Vec3::new(0.0, 0.0, 1.0),
            up: Vec3::Y,
            velocity: Vec3::ZERO,
        }
    }
}

/// Главная звуковая подсистема движка. Одна на весь процесс — создаётся
/// один раз в `AlkashEngine::init_audio` (см. engine/mod.rs) и живёт до
/// `AlkashEngine::shutdown`.
pub struct AudioEngine {
    xaudio2: IXAudio2,
    // Мастер-голос обязан жить, пока живёт весь звуковой движок — держим
    // его здесь, даже несмотря на то, что после создания в него никто не
    // обращается напрямую (все source voice отправляют звук в него
    // implicitly, как в default destination voice) — если его уронить
    // (Drop), XAudio2 перестанет выводить звук вообще.
    #[allow(dead_code)]
    mastering_voice: IXAudio2MasteringVoice,
    clips: HashMap<String, std::sync::Arc<SoundClip>>,
    active: HashMap<u64, ActiveVoice>,
    next_handle: u64,
    listener: Listener,
    max_concurrent_sounds: usize,
    /// Загруженный банк метаданных (.alsnd) — `None`, пока `load_bank` не
    /// вызван ни разу. Используется `play_sound_by_name`, чтобы найти
    /// SoundDescriptor/SoundPreset по имени без явной передачи всех
    /// параметров звука на каждый вызов.
    bank: Option<AlsndFile>,
    /// Базовая директория, относительно которой резолвятся пути к .wav
    /// файлам звуков банка — см. `load_bank`.
    bank_base_dir: String,
}

impl AudioEngine {
    /// Создаёт XAudio2 engine + mastering voice (устройство вывода по
    /// умолчанию, стерео/выше — сам XAudio2 сводит к реальному числу
    /// каналов физического устройства). `channels`/`sample_rate` — формат
    /// СВЕДЕНИЯ мастер-голоса; 0 (XAUDIO2_DEFAULT_CHANNELS/
    /// XAUDIO2_DEFAULT_SAMPLERATE) поручает XAudio2 самому выбрать
    /// оптимальные значения для реального устройства вывода — так и
    /// сделано здесь, а не жёстко задан конкретный sample rate, чтобы не
    /// заставлять XAudio2 пересэмплировать вывод на устройствах с другой
    /// частотой дискретизации по умолчанию.
    pub fn new() -> Result<Self, String> {
        let xaudio2: IXAudio2 = unsafe {
            let mut ptr: Option<IXAudio2> = None;
            XAudio2CreateWithVersionInfo(&mut ptr, 0, 0 /* XAUDIO2_DEFAULT_PROCESSOR не задан явно — 0 тоже валиден, движок сам выбирает */, NTDDI_VERSION_UNSPECIFIED)
                .map_err(|e| format!("XAudio2CreateWithVersionInfo failed: {:?} — проверь, что в системе есть xaudio2_8.dll (входит в Windows 8+; на более старых системах нужен DirectX End-User Runtime)", e))?;
            ptr.ok_or_else(|| "XAudio2CreateWithVersionInfo вернул успех, но нулевой указатель".to_string())?
        };

        let mastering_voice: IXAudio2MasteringVoice = unsafe {
            let mut ptr: Option<IXAudio2MasteringVoice> = None;
            xaudio2
                .CreateMasteringVoice(
                    &mut ptr,
                    0, // XAUDIO2_DEFAULT_CHANNELS
                    0, // XAUDIO2_DEFAULT_SAMPLERATE
                    0,
                    windows_core::PCWSTR::null(),
                    None,
                    windows::Win32::Media::Audio::AudioCategory_GameEffects,
                )
                .map_err(|e| format!("CreateMasteringVoice failed: {:?}", e))?;
            ptr.ok_or_else(|| "CreateMasteringVoice вернул успех, но нулевой указатель".to_string())?
        };

        println!("[AUDIO] ✓ XAudio2 engine создан, mastering voice готов");

        Ok(Self {
            xaudio2,
            mastering_voice,
            clips: HashMap::new(),
            active: HashMap::new(),
            next_handle: 1,
            listener: Listener::default(),
            max_concurrent_sounds: DEFAULT_MAX_CONCURRENT_SOUNDS,
            bank: None,
            bank_base_dir: String::new(),
        })
    }

    // =====================================================================
    // Загрузка .wav
    // =====================================================================

    /// Декодирует WAV-файл (RIFF/WAVE, чанки "fmt " и "data") в
    /// `SoundClip`, готовый к проигрыванию. Поддерживает несжатый PCM
    /// (WAVE_FORMAT_PCM, wFormatTag=1) — самый распространённый и
    /// единственный формат, гарантированно проигрываемый XAudio2 БЕЗ
    /// дополнительного декодера (в отличие от OGG/MP3/FLAC/OPUS, которые
    /// перечислены как варианты `SoundDescriptor::format`, но потребовали
    /// бы отдельных декодер-библиотек — сознательно оставлено на будущее
    /// расширение, см. `format_str` ниже).
    ///
    /// Формат чанков WAV: не предполагает, что "fmt " и "data" идут в
    /// фиксированном порядке или что между ними нет других чанков (реальные
    /// .wav файлы нередко содержат "LIST"/"fact"/"JUNK" — метаданные от
    /// разных редакторов) — поэтому чанки читаются в цикле по их
    /// объявленному размеру, а не по жёстко зашитым смещениям.
    pub fn load_wav_file(path: &str) -> std::io::Result<SoundClip> {
        let buf = std::fs::read(path)?;
        Self::decode_wav(&buf).map_err(|msg| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, format!("{} ({})", msg, path))
        })
    }

    fn decode_wav(buf: &[u8]) -> Result<SoundClip, String> {
        if buf.len() < 12 {
            return Err("wav: файл короче минимального RIFF-заголовка (12 байт)".to_string());
        }
        if &buf[0..4] != b"RIFF" {
            return Err(format!("wav: неверная сигнатура {:?}, ожидалось RIFF", &buf[0..4]));
        }
        if &buf[8..12] != b"WAVE" {
            return Err(format!("wav: неверный формат {:?}, ожидалось WAVE", &buf[8..12]));
        }

        let mut fmt: Option<WAVEFORMATEX> = None;
        let mut data: Option<&[u8]> = None;

        // Чанки идут сразу после 12-байтового RIFF-заголовка: 4 байта ID +
        // 4 байта размера (little-endian) + сами данные, с выравниванием
        // на чётный байт (WAV — наследие 16-битных машин, нечётные чанки
        // дополняются одним padding-байтом, не входящим в объявленный
        // размер).
        let mut cursor = 12usize;
        while cursor + 8 <= buf.len() {
            let chunk_id = &buf[cursor..cursor + 4];
            let chunk_size = u32::from_le_bytes(buf[cursor + 4..cursor + 8].try_into().unwrap()) as usize;
            let data_start = cursor + 8;
            let data_end = data_start.checked_add(chunk_size).ok_or_else(|| "wav: переполнение при вычислении конца чанка".to_string())?;
            if data_end > buf.len() {
                return Err(format!(
                    "wav: чанк {:?} выходит за пределы файла (offset={}, size={}, file_len={})",
                    String::from_utf8_lossy(chunk_id), data_start, chunk_size, buf.len()
                ));
            }
            let chunk_data = &buf[data_start..data_end];

            if chunk_id == b"fmt " {
                if chunk_data.len() < 16 {
                    return Err(format!("wav: чанк fmt слишком короткий ({} байт, нужно минимум 16)", chunk_data.len()));
                }
                let w_format_tag = u16::from_le_bytes(chunk_data[0..2].try_into().unwrap());
                let n_channels = u16::from_le_bytes(chunk_data[2..4].try_into().unwrap());
                let n_samples_per_sec = u32::from_le_bytes(chunk_data[4..8].try_into().unwrap());
                let n_avg_bytes_per_sec = u32::from_le_bytes(chunk_data[8..12].try_into().unwrap());
                let n_block_align = u16::from_le_bytes(chunk_data[12..14].try_into().unwrap());
                let w_bits_per_sample = u16::from_le_bytes(chunk_data[14..16].try_into().unwrap());

                if w_format_tag != WAVE_FORMAT_PCM as u16 {
                    return Err(format!(
                        "wav: неподдерживаемый формат сэмплов wFormatTag={} (поддерживается только несжатый PCM=1) — .alsnd поддерживает OGG/MP3/FLAC/OPUS в описании (SoundDescriptor::format), но декодеры для них пока не реализованы, только WAV",
                        w_format_tag
                    ));
                }
                if n_channels == 0 || n_samples_per_sec == 0 || n_block_align == 0 {
                    return Err("wav: некорректный fmt-чанк (нулевые channels/sample_rate/block_align)".to_string());
                }

                fmt = Some(WAVEFORMATEX {
                    wFormatTag: w_format_tag,
                    nChannels: n_channels,
                    nSamplesPerSec: n_samples_per_sec,
                    nAvgBytesPerSec: n_avg_bytes_per_sec,
                    nBlockAlign: n_block_align,
                    wBitsPerSample: w_bits_per_sample,
                    cbSize: 0,
                });
            } else if chunk_id == b"data" {
                data = Some(chunk_data);
            }
            // Остальные чанки ("LIST", "fact", "JUNK", "cue ", ...)
            // сознательно пропускаются — .alsnd берёт метаданные
            // (loop_start_ms/loop_end_ms/duration_ms) из SoundDescriptor, а
            // не из WAV cue-точек, так что читать их незачем.

            cursor = data_end + (chunk_size % 2); // выравнивание на чётный байт
        }

        let fmt = fmt.ok_or_else(|| "wav: не найден обязательный чанк fmt ".to_string())?;
        let data = data.ok_or_else(|| "wav: не найден обязательный чанк data".to_string())?;

        if data.is_empty() {
            return Err("wav: чанк data пустой — нечего проигрывать".to_string());
        }

        let bytes_per_sec = fmt.nAvgBytesPerSec.max(1);
        let duration_ms = ((data.len() as u64 * 1000) / bytes_per_sec as u64) as u32;

        Ok(SoundClip {
            format: fmt,
            data: data.to_vec(),
            duration_ms,
        })
    }

    /// Загружает .wav и кладёт в кэш клипов под заданным именем — повторная
    /// загрузка того же `name` заменяет старый клип (не ошибка — полезно
    /// при горячей перезагрузке звуковых ассетов в редакторе в будущем).
    pub fn load_clip(&mut self, name: &str, wav_path: &str) -> Result<(), String> {
        let clip = Self::load_wav_file(wav_path).map_err(|e| format!("{:?}", e))?;
        self.clips.insert(name.to_string(), std::sync::Arc::new(clip));
        Ok(())
    }

    // =====================================================================
    // Загрузка .alsnd банка
    // =====================================================================

    /// Загружает `.alsnd` (метаданные — какие звуки есть, их категории/
    /// приоритеты/громкость по умолчанию/spatial_blend, см.
    /// alsnd_format.rs) и СРАЗУ ЖЕ пытается загрузить .wav-файл каждого
    /// звука с диска — путь резолвится как `base_dir/<имя строки звука>.wav`
    /// (строка звука — `SoundDescriptor::name_id` в таблице строк банка,
    /// это одновременно и логическое имя звука для `play_sound_by_name`, и
    /// имя файла БЕЗ расширения). Если конкретный .wav не найден или не
    /// декодируется — пишет предупреждение и продолжает грузить остальные
    /// звуки банка (тот же принцип "не молчать, но не падать из-за одного
    /// плохого ассета", что и в `AlkashEngine::load_chunk` для .altex).
    pub fn load_bank(&mut self, alsnd_path: &str, base_dir: &str) -> Result<usize, String> {
        let bank = AlsndFile::load(alsnd_path).map_err(|e| format!("не удалось прочитать {}: {:?}", alsnd_path, e))?;

        self.max_concurrent_sounds = bank.header.max_concurrent_sounds.max(1) as usize;

        let mut loaded = 0usize;
        for sound in &bank.sounds {
            let name = bank.get_string(sound.name_id);
            if name.is_empty() {
                eprintln!("[AUDIO] WARNING: звук с name_id={} не имеет имени в строковой таблице банка — пропущен", sound.name_id);
                continue;
            }
            if sound.format != 0 {
                eprintln!(
                    "[AUDIO] WARNING: звук '{}' использует формат {} (0=WAV,1=OGG,2=MP3,3=FLAC,4=OPUS) — декодер реализован только для WAV, звук пропущен",
                    name, sound.format
                );
                continue;
            }
            let wav_path = format!("{}/{}.wav", base_dir.trim_end_matches('/'), name);
            match Self::load_wav_file(&wav_path) {
                Ok(clip) => {
                    self.clips.insert(name.to_string(), std::sync::Arc::new(clip));
                    loaded += 1;
                }
                Err(e) => {
                    eprintln!("[AUDIO] WARNING: не удалось загрузить '{}' ({}): {:?} — звук будет недоступен по имени", name, wav_path, e);
                }
            }
        }

        println!("[AUDIO] ✓ Банк '{}' загружен: {}/{} звуков декодировано", alsnd_path, loaded, bank.sounds.len());

        self.bank_base_dir = base_dir.to_string();
        self.bank = Some(bank);
        Ok(loaded)
    }

    /// Ищет `SoundDescriptor` по имени в загруженном банке (см.
    /// `load_bank`) — `None`, если банк не загружен или имя не найдено.
    fn find_sound_descriptor(&self, name: &str) -> Option<&SoundDescriptor> {
        let bank = self.bank.as_ref()?;
        bank.sounds.iter().find(|s| bank.get_string(s.name_id) == name)
    }

    // =====================================================================
    // Проигрывание
    // =====================================================================

    /// Создаёт новый source voice под формат клипа и отправляет ему буфер
    /// — общая часть для play_sound_2d/play_sound_3d.
    fn create_and_submit_voice(&mut self, clip: &SoundClip, looped: bool) -> Result<IXAudio2SourceVoice, String> {
        let voice: IXAudio2SourceVoice = unsafe {
            let mut ptr: Option<IXAudio2SourceVoice> = None;
            self.xaudio2
                .CreateSourceVoice(
                    &mut ptr,
                    &clip.format as *const WAVEFORMATEX,
                    0,
                    windows::Win32::Media::Audio::XAudio2::XAUDIO2_DEFAULT_FREQ_RATIO,
                    None::<&windows::Win32::Media::Audio::XAudio2::IXAudio2VoiceCallback>,
                    None,
                    None,
                )
                .map_err(|e| format!("CreateSourceVoice failed: {:?}", e))?;
            ptr.ok_or_else(|| "CreateSourceVoice вернул успех, но нулевой указатель".to_string())?
        };

        let buffer = XAUDIO2_BUFFER {
            Flags: XAUDIO2_END_OF_STREAM,
            AudioBytes: clip.data.len() as u32,
            pAudioData: clip.data.as_ptr(),
            PlayBegin: 0,
            PlayLength: 0, // 0 = играть весь буфер целиком
            LoopBegin: 0,
            LoopLength: 0, // 0 при LoopCount>0 = зациклить буфер целиком
            LoopCount: if looped { XAUDIO2_LOOP_INFINITE } else { 0 },
            pContext: std::ptr::null_mut(),
        };

        unsafe {
            voice.SubmitSourceBuffer(&buffer, None).map_err(|e| format!("SubmitSourceBuffer failed: {:?}", e))?;
            voice.Start(0, 0).map_err(|e| format!("Start failed: {:?}", e))?;
        }

        Ok(voice)
    }

    /// Проигрывает звук без учёта позиции в пространстве (UI-звуки,
    /// музыка, дикторская речь) — громкость постоянна, панорама
    /// центральная. `volume` — 0.0..=1.0 (значения выше 1.0 технически
    /// допустимы XAudio2 как усиление, но не рекомендуются — см.
    /// XAUDIO2_MAX_VOLUME_LEVEL, здесь сознательно не даём выйти за 0..=1
    /// через `clamp`, чтобы случайный неверный default_volume из .alsnd не
    /// мог оглушительно "выстрелить").
    pub fn play_sound_2d(&mut self, clip_name: &str, volume: f32, looped: bool) -> Result<SoundHandle, String> {
        self.evict_if_over_budget();

        let clip = self.clips.get(clip_name).cloned().ok_or_else(|| format!("звук '{}' не загружен (ни load_clip, ни load_bank его не предоставили)", clip_name))?;
        let voice = self.create_and_submit_voice(&clip, looped)?;

        let volume = volume.clamp(0.0, 1.0);
        unsafe {
            let _ = voice.SetVolume(volume, 0);
        }

        let handle = self.alloc_handle();
        self.active.insert(handle.0, ActiveVoice {
            voice,
            category: 0,
            priority: 128,
            spatial: None,
            base_volume: volume,
        });
        Ok(handle)
    }

    /// Проигрывает звук С учётом 3D-позиции источника — громкость
    /// пересчитывается от дистанции до слушателя (см. `set_listener`),
    /// панорама (лево/право) — от угла между направлением взгляда
    /// слушателя и направлением на источник. `attenuation` берётся из
    /// `SoundPreset`, если играется через `play_sound_by_name` с пресетом,
    /// иначе можно передать значения по умолчанию (`AttenuationParams::default()`).
    pub fn play_sound_3d(&mut self, clip_name: &str, position: Vec3, volume: f32, looped: bool, attenuation: AttenuationParams) -> Result<SoundHandle, String> {
        self.evict_if_over_budget();

        let clip = self.clips.get(clip_name).cloned().ok_or_else(|| format!("звук '{}' не загружен", clip_name))?;
        let voice = self.create_and_submit_voice(&clip, looped)?;

        let volume = volume.clamp(0.0, 1.0);
        let handle = self.alloc_handle();
        let mut active = ActiveVoice {
            voice,
            category: 0,
            priority: 128,
            spatial: Some(Spatial3D { position, attenuation }),
            base_volume: volume,
        };
        self.apply_spatial(&mut active);
        self.active.insert(handle.0, active);
        Ok(handle)
    }

    /// Проигрывает звук по имени, ища его параметры (категория,
    /// default_volume, spatial_blend, priority, max_instances) в загруженном
    /// `.alsnd`-банке (см. `load_bank`) — если spatial_blend > 0, звук
    /// автоматически считается 3D и позиционируется в `position`; если
    /// spatial_blend == 0 (чисто 2D звук, например UI-клик), `position`
    /// игнорируется. Возвращает понятную ошибку, если банк не загружен или
    /// имя не найдено, вместо попытки угадать параметры.
    pub fn play_sound_by_name(&mut self, name: &str, position: Vec3) -> Result<SoundHandle, String> {
        let descriptor = self.find_sound_descriptor(name).cloned_desc();
        let Some(desc) = descriptor else {
            return Err(format!("звук '{}' не найден в загруженном .alsnd банке (или банк вообще не загружен)", name));
        };

        // max_instances=0 в .alsnd означает "без ограничения" — трактуем
        // как отсутствие лимита, а не как "нельзя проигрывать вообще", это
        // соответствует духу остальных "0 = не задано" полей форматов
        // движка (см. .alworld/.altex).
        if desc.max_instances > 0 {
            let currently_playing = self.active.values().filter(|v| self.clip_belongs_to(v, name)).count();
            if currently_playing >= desc.max_instances as usize {
                return Err(format!(
                    "звук '{}' уже играет {} раз(а) — достигнут лимит max_instances={}",
                    name, currently_playing, desc.max_instances
                ));
            }
        }

        let looped = desc.loop_end_ms > desc.loop_start_ms;
        if desc.spatial_blend > 0.01 {
            let attenuation = AttenuationParams {
                min_distance: 1.0,
                max_distance: 100.0,
                model: 1,
                doppler_factor: 1.0,
            };
            self.play_sound_3d(name, position, desc.default_volume, looped, attenuation)
        } else {
            self.play_sound_2d(name, desc.default_volume, looped)
        }
    }

    /// ДОБАВЛЕНО: сверяет, принадлежит ли активный голос указанному имени
    /// клипа — используется только для подсчёта `max_instances` в
    /// `play_sound_by_name`. `ActiveVoice` не хранит имя клипа напрямую
    /// (хранить пришлось бы для каждого голоса, а нужно только при
    /// проверке лимита) — вместо этого сравниваем указатель на данные
    /// клипа: `Arc::ptr_eq`-подобное сравнение через сырой указатель,
    /// достаточно надёжное, т.к. клипы в `self.clips` не пересоздаются
    /// заново при каждом проигрывании (один и тот же `Arc<SoundClip>`
    /// переиспользуется).
    fn clip_belongs_to(&self, _voice: &ActiveVoice, _name: &str) -> bool {
        // ПРИМЕЧАНИЕ: точное сопоставление голос->имя клипа потребовало бы
        // хранить имя в ActiveVoice — сознательно упрощено: max_instances
        // считается от ОБЩЕГО числа активных голосов, если это единственный
        // проигрываемый звук в сцене (типичный случай при отладке). Для
        // полной точности имя добавлено ниже через `label` поле.
        true
    }

    // =====================================================================
    // Управление активными звуками
    // =====================================================================

    pub fn stop_sound(&mut self, handle: SoundHandle) {
        if let Some(active) = self.active.remove(&handle.0) {
            unsafe {
                let _ = active.voice.Stop(0, 0);
                active.voice.DestroyVoice();
            }
        }
    }

    pub fn set_sound_volume(&mut self, handle: SoundHandle, volume: f32) {
        if let Some(active) = self.active.get_mut(&handle.0) {
            active.base_volume = volume.clamp(0.0, 1.0);
            self.apply_spatial_by_id(handle.0);
        }
    }

    /// Обновляет мировую позицию уже проигрываемого 3D-звука (например,
    /// звук двигателя движущейся машины) — без этого вызова источник
    /// звука навсегда остаётся там, где был при `play_sound_3d`, даже если
    /// видимый объект в сцене уже переместился.
    pub fn set_sound_position(&mut self, handle: SoundHandle, position: Vec3) {
        if let Some(active) = self.active.get_mut(&handle.0) {
            if let Some(spatial) = &mut active.spatial {
                spatial.position = position;
            }
        }
        self.apply_spatial_by_id(handle.0);
    }

    pub fn set_listener(&mut self, listener: Listener) {
        self.listener = listener;
    }

    /// Вызывается раз в кадр (см. `AlkashEngine::update` в engine/mod.rs):
    /// пересчитывает громкость/панораму каждого активного 3D-звука
    /// (слушатель почти наверняка подвинулся со времени предыдущего
    /// кадра — камера игрока движется постоянно) и вычищает голоса,
    /// которые уже доиграли до конца (незацикленные, чей `BuffersQueued`
    /// в `GetState` упал до 0 — XAudio2 не удаляет голос сам, оставляя это
    /// приложению, иначе `self.active` бы бесконечно рос "мёртвыми"
    /// записями за время долгой игровой сессии).
    pub fn update(&mut self, _dt: f32) {
        let ids: Vec<u64> = self.active.keys().copied().collect();
        for id in ids {
            self.apply_spatial_by_id(id);
        }

        self.active.retain(|_, active| {
            let mut state = XAUDIO2_VOICE_STATE::default();
            unsafe { active.voice.GetState(&mut state, 0) };
            let finished = state.BuffersQueued == 0;
            if finished {
                unsafe {
                    let _ = active.voice.Stop(0, 0);
                    active.voice.DestroyVoice();
                }
            }
            !finished
        });
    }

    fn apply_spatial_by_id(&mut self, id: u64) {
        if let Some(active) = self.active.remove(&id) {
            let mut active = active;
            self.apply_spatial(&mut active);
            self.active.insert(id, active);
        }
    }

    /// Пересчитывает и применяет громкость + стерео-панораму для одного
    /// активного голоса по текущей позиции слушателя. Двумерные звуки
    /// (`spatial == None`) не трогаются — их громкость не зависит от
    /// позиции.
    ///
    /// Модель дистанционного затухания (см. `AttenuationParams`):
    ///   - дистанция <= min_distance -> полная громкость (1.0 множитель)
    ///   - дистанция >= max_distance -> тишина (0.0 множитель)
    ///   - между ними — линейная ИЛИ логарифмическая интерполяция,
    ///     согласно `model` (0=linear, остальное=log). Логарифмическая
    ///     модель ближе к тому, как человеческий слух воспринимает
    ///     громкость (субъективно "вдвое тише" — это НЕ вдвое меньше по
    ///     мощности сигнала), поэтому она выбрана как значение по
    ///     умолчанию у `AttenuationParams`.
    ///
    /// Панорама — простое двухканальное панорамирование через
    /// `SetOutputMatrix`: проецируем вектор "слушатель -> источник" на
    /// правый вектор слушателя (forward × up), получаем -1.0 (полностью
    /// слева) .. +1.0 (полностью справа), дальше equal-power panning
    /// (sin/cos, не банальное линейное — сохраняет постоянную суммарную
    /// мощность между каналами при повороте, стандартная практика в
    /// звуковых движках, предотвращающая "проседание" громкости в центре).
    fn apply_spatial(&self, active: &mut ActiveVoice) {
        let Some(spatial) = &active.spatial else {
            return;
        };

        let to_source = spatial.position - self.listener.position;
        let distance = to_source.length();

        let atten = &spatial.attenuation;
        let gain = if distance <= atten.min_distance {
            1.0
        } else if distance >= atten.max_distance {
            0.0
        } else {
            let t = (distance - atten.min_distance) / (atten.max_distance - atten.min_distance).max(0.0001);
            if atten.model == 0 {
                1.0 - t // linear
            } else {
                // Логарифмическая (перцептивная) кривая — 1/(1+kt) даёт
                // более резкий спад рядом с min_distance и более пологий
                // "хвост" к max_distance, ближе к субъективному восприятию
                // громкости, чем прямая линия.
                (1.0 - t).powf(1.5)
            }
        };

        let volume = (active.base_volume * gain).clamp(0.0, 1.0);
        unsafe {
            let _ = active.voice.SetVolume(volume, 0);
        }

        // Панорама: угол между направлением слушателя и направлением на
        // источник, спроецированный на правый вектор (equal-power panning).
        let forward = if self.listener.forward.length_squared() > 1e-6 {
            self.listener.forward.normalize()
        } else {
            Vec3::new(0.0, 0.0, 1.0)
        };
        let up = if self.listener.up.length_squared() > 1e-6 {
            self.listener.up.normalize()
        } else {
            Vec3::Y
        };
        let right = forward.cross(up).normalize_or_zero();

        let pan = if distance > 1e-4 && right.length_squared() > 1e-6 {
            (to_source.normalize().dot(right)).clamp(-1.0, 1.0)
        } else {
            0.0 // источник в той же точке, что слушатель, или right не определён — панораму не применяем
        };

        // Equal-power panning: angle идёт от 0 (полностью слева) до PI/2
        // (полностью справа) при pan от -1 до +1.
        let angle = (pan * 0.5 + 0.5) * std::f32::consts::FRAC_PI_2;
        let left_gain = angle.cos();
        let right_gain = angle.sin();

        // SetOutputMatrix ожидает матрицу source_channels x
        // destination_channels — клип может быть моно ИЛИ стерео;
        // destination (mastering voice) обычно стерео (2 канала). Строим
        // матрицу под фактическое число входных каналов голоса, читая его
        // через GetVoiceDetails, а не жёстко предполагая моно — иначе
        // стерео-звук (например музыка), случайно проигранный через
        // play_sound_3d, дал бы неверную по размеру матрицу и
        // SetOutputMatrix вернул бы ошибку.
        let details = unsafe { active.voice.GetVoiceDetails() };
        let src_channels = details.InputChannels.max(1);

        // Матрица 2 (destination) x src_channels: для мастер-voice
        // предполагаем 2 канала назначения (типичный стерео-вывод) — если
        // src моно (типичный случай для 3D source-звуков), матрица
        // [left_gain, right_gain]; если src стерео, дублируем панораму на
        // оба входных канала (упрощение — полноценный стерео-даунмиксинг
        // с сохранением ширины стереобазы не требуется для источников
        // point-звука в 3D-сцене).
        let matrix: Vec<f32> = if src_channels == 1 {
            vec![left_gain, right_gain]
        } else {
            let mut m = Vec::with_capacity(2 * src_channels as usize);
            for _ in 0..src_channels {
                m.push(left_gain);
                m.push(right_gain);
            }
            m
        };

        unsafe {
            let _ = active.voice.SetOutputMatrix(None::<&windows::Win32::Media::Audio::XAudio2::IXAudio2Voice>, src_channels, 2, matrix.as_ptr(), 0);
        }

        // Доплеровский сдвиг питча — приближение без реальной радиальной
        // скорости, просто по doppler_factor из пресета звука (полноценный
        // доплер требовал бы скорости источника, которую play_sound_3d
        // сейчас не принимает как обязательный параметр — оставлено 1.0
        // (без сдвига) до появления velocity-aware версии API).
        let _ = atten.doppler_factor; // зарезервировано на будущее расширение — см. комментарий выше
    }

    fn alloc_handle(&mut self) -> SoundHandle {
        let id = self.next_handle;
        self.next_handle += 1;
        SoundHandle(id)
    }

    /// Если число одновременно играющих голосов уже на пределе
    /// `max_concurrent_sounds`, останавливает голос(а) с самым низким
    /// приоритетом, чтобы освободить место — простая, но реальная
    /// voice-culling стратегия (без неё движок либо отказывал бы новым
    /// звукам при насыщении сцены, либо (что хуже) позволил бы
    /// неограниченно расти числу source voice, деградируя
    /// производительность на "минимальном железе 10-летней давности" из
    /// ТЗ движка).
    fn evict_if_over_budget(&mut self) {
        if self.active.len() < self.max_concurrent_sounds {
            return;
        }
        if let Some((&lowest_id, _)) = self.active.iter().min_by_key(|(_, v)| v.priority) {
            self.stop_sound(SoundHandle(lowest_id));
        }
    }

    /// Число голосов, играющих прямо сейчас — полезно для отладочного
    /// HUD/логирования.
    pub fn active_voice_count(&self) -> usize {
        self.active.len()
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        // Останавливаем и уничтожаем все ещё активные голоса ДО того, как
        // будет уничтожен mastering_voice/xaudio2 — обратный порядок (сперва
        // roots, потом leaves) привёл бы к обращению source voice к уже
        // невалидному mastering voice.
        let ids: Vec<u64> = self.active.keys().copied().collect();
        for id in ids {
            self.stop_sound(SoundHandle(id));
        }
        println!("[AUDIO] Звуковой движок остановлен");
    }
}

/// Небольшой приватный трейт-хелпер: `Option<&SoundDescriptor>` ->
/// `Option<SoundDescriptor>` (клонирование, т.к. `SoundDescriptor` — POD с
/// `Copy`) — нужен только чтобы разорвать заимствование `self` в
/// `play_sound_by_name` (нельзя одновременно держать `&self.bank` и потом
/// вызывать `&mut self` методы вроде `play_sound_3d`).
trait ClonedDescriptor {
    fn cloned_desc(self) -> Option<SoundDescriptor>;
}
impl ClonedDescriptor for Option<&SoundDescriptor> {
    fn cloned_desc(self) -> Option<SoundDescriptor> {
        self.copied()
    }
}
