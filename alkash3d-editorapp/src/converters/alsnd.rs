// src/converters/alsnd.rs
//
// Экспорт AudioSource-объектов сцены в настоящий `.alsnd` движка
// (alkash3d_rs::AlsndFile) как один SoundBank с дескрипторами звуков.
//
// ИЗВЕСТНОЕ ОГРАНИЧЕНИЕ (формата, не эдитора): `AlsndFile::save()` в
// движке сериализует только МЕТАДАННЫЕ (`SoundDescriptor` — offset/размер/
// громкость/приоритет и т.п.), а не сырые байты аудио — `data_offset`/
// `size_compressed` полей `SoundDescriptor` пишутся нулями, реальный блок
// байтов в файл не попадает вообще (см. `AlsndFile::save` в
// alsnd_format.rs — там нет ни одного `file.write_all` с аудио-данными).
// Так что `.alsnd`, экспортированный отсюда, описывает НАБОР именованных
// звуковых слотов сцены, а не переносит сам звук — соответствует текущему
// реальному состоянию формата в движке, а не воображаемому.

use anyhow::{anyhow, Result};

use crate::scene::{AudioSourceComponent, GameObject, ObjectType, Scene};

use alkash3d_rs::{AlsndFile, SoundBank, SoundDescriptor};

// ДОБАВЛЕНО (по прямому запросу пользователя: "давай делать эдитор под
// каждый формат... чтобы они не лежали мёртвым грузом"): `.alsnd` — не
// пространственные данные (см. шапку файла — `SoundDescriptor` вообще не
// хранит позицию), так что "поместить звук в 3D-сцену" (как раньше —
// `AudioSourceComponent`) не лучший способ его РЕДАКТИРОВАТЬ — по сути это
// просто именованный список звуковых слотов с параметрами, тем и является
// `SoundEntryEdit` ниже: прямое, ничем не опосредованное представление
// одного `SoundDescriptor`, которое держит `ui/sound_bank_editor.rs` в
// `Vec<SoundEntryEdit>` (не привязано к `Scene`/объектам вообще). Хранит
// ВСЕ поля дескриптора, которые есть смысл настраивать руками (формат/
// категория/громкость/питч/приоритет/макс.копий/spatial_blend) — в отличие
// от `export_scene_to_alsnd` выше, который часть из них (`format`,
// `priority`, `max_instances`) просто жёстко прибивал плейсхолдерами,
// потому что `AudioSourceComponent` этих полей не хранит.
#[derive(Debug, Clone)]
pub struct SoundEntryEdit {
    pub name: String,
    /// 0=WAV, 1=OGG, 2=MP3, 3=FLAC, 4=OPUS — см. `SoundDescriptor::format`.
    pub format: u32,
    /// 0=SFX, 1=Music, 2=Ambient, 3=Voice, 4=UI.
    pub category: u32,
    pub volume: f32,
    pub pitch: f32,
    pub priority: u32,
    pub max_instances: u32,
    pub spatial_blend: f32,
}

impl Default for SoundEntryEdit {
    fn default() -> Self {
        Self {
            name: String::new(),
            format: 0,
            category: 0,
            volume: 1.0,
            pitch: 1.0,
            priority: 128,
            max_instances: 4,
            spatial_blend: 1.0,
        }
    }
}

/// Собирает `AlsndFile` напрямую из отредактированных записей — записи с
/// пустым именем молча пропускаются (пустая строка-заготовка после "+ Add
/// Sound", которую ещё не заполнили, не должна попасть в сохранённый файл).
pub fn build_alsnd_file(bank_name: &str, entries: &[SoundEntryEdit]) -> AlsndFile {
    let mut file = AlsndFile::new(2, 48000);
    for e in entries {
        let name = e.name.trim();
        if name.is_empty() {
            continue;
        }
        let name_id = file.add_string(name);
        file.sounds.push(SoundDescriptor {
            name_id,
            format: e.format,
            category: e.category,
            data_offset: 0,
            size_compressed: 0,
            size_uncompressed: 0,
            duration_ms: 0,
            loop_start_ms: 0,
            loop_end_ms: 0,
            default_volume: e.volume,
            default_pitch: e.pitch,
            priority: e.priority,
            max_instances: e.max_instances,
            spatial_blend: e.spatial_blend,
        });
    }
    if !file.sounds.is_empty() {
        let bank_name_id = file.add_string(bank_name);
        file.banks.push(SoundBank {
            name_id: bank_name_id,
            sounds_start: 0,
            sounds_count: file.sounds.len() as u32,
            preload_all: 1,
            keep_in_memory: 0,
            memory_budget_mb: 64,
        });
    }
    file
}

pub fn save_sound_bank(bank_name: &str, entries: &[SoundEntryEdit], path: &str) -> Result<usize> {
    let file = build_alsnd_file(bank_name, entries);
    let count = file.sounds.len();
    if count == 0 {
        return Err(anyhow!("Нет ни одного звука с именем — сохранять нечего"));
    }
    file.save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alsnd '{}': {}", path, e))?;
    Ok(count)
}

/// Обратное к `build_alsnd_file` — грузит `.alsnd` прямо в редактируемые
/// записи (для "📂 Load..." в `SoundBankEditor`), а не в объекты сцены (см.
/// `import_alsnd_to_scene` ниже, для другого сценария использования — тот
/// продолжает работать как раньше). Имя банка — из ПЕРВОГО `SoundBank`
/// файла (обычно он один, см. `build_alsnd_file`); пусто, если банков нет.
pub fn load_sound_bank(path: &str) -> Result<(String, Vec<SoundEntryEdit>)> {
    let file = AlsndFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alsnd '{}': {}", path, e))?;
    let bank_name = file.banks.first().map(|b| file.get_string(b.name_id).to_string()).unwrap_or_default();
    let entries = file.sounds.iter().map(|s| SoundEntryEdit {
        name: file.get_string(s.name_id).to_string(),
        format: s.format,
        category: s.category,
        volume: s.default_volume,
        pitch: s.default_pitch,
        priority: s.priority,
        max_instances: s.max_instances,
        spatial_blend: s.spatial_blend,
    }).collect();
    Ok((bank_name, entries))
}

pub fn export_scene_to_alsnd(scene: &Scene) -> AlsndFile {
    let mut file = AlsndFile::new(2, 48000);
    let mut sound_indices = std::collections::HashMap::new();

    for obj in scene.objects.values() {
        let ObjectType::AudioSource(a) = &obj.object_type else { continue };
        if !a.enabled || a.sound_name.is_empty() {
            continue;
        }
        sound_indices.entry(a.sound_name.clone()).or_insert_with(|| {
            let name_id = file.add_string(&a.sound_name);
            let idx = file.sounds.len() as u32;
            file.sounds.push(SoundDescriptor {
                name_id,
                format: 0,   // WAV — плейсхолдер по умолчанию, реального аудио файл не несёт (см. шапку)
                category: 0, // SFX
                data_offset: 0,
                size_compressed: 0,
                size_uncompressed: 0,
                duration_ms: 0,
                loop_start_ms: 0,
                loop_end_ms: 0,
                default_volume: a.volume,
                default_pitch: 1.0,
                priority: 128,
                max_instances: 4,
                spatial_blend: a.spatial_blend,
            });
            idx
        });
    }

    if !file.sounds.is_empty() {
        let bank_name_id = file.add_string("SceneAudio");
        file.banks.push(SoundBank {
            name_id: bank_name_id,
            sounds_start: 0,
            sounds_count: file.sounds.len() as u32,
            preload_all: 1,
            keep_in_memory: 0,
            memory_budget_mb: 64,
        });
    }

    file
}

pub fn export_scene_to_alsnd_file(scene: &Scene, path: &str) -> Result<usize> {
    let file = export_scene_to_alsnd(scene);
    let count = file.sounds.len();
    if count == 0 {
        return Err(anyhow!("В сцене нет включённых AudioSource-объектов с именем звука — экспортировать нечего"));
    }
    file.save(path)
        .map_err(|e| anyhow!("Не удалось сохранить .alsnd '{}': {}", path, e))?;
    Ok(count)
}

/// Импорт `.alsnd` — по одному `AudioSource`-объекту сцены на каждый
/// `SoundDescriptor` из ВСЕХ банков файла (сам банк — просто группировка
/// имён, у AudioSource в сцене эдитора нет понятия "банк", см. `ObjectType`
/// — так что группировка при импорте не сохраняется, только плоский набор
/// звуков, зеркально тому, что реально читает `export_scene_to_alsnd` выше).
/// Позиции у объектов нет смысла восстанавливать — `SoundDescriptor` её не
/// хранит (см. шапку файла про то, что формат вообще не про 3D-размещение).
pub fn import_alsnd_to_scene(path: &str, log: &mut dyn FnMut(String)) -> Result<Scene> {
    let file = AlsndFile::load(path).map_err(|e| anyhow!("Не удалось прочитать .alsnd '{}': {}", path, e))?;
    if file.sounds.is_empty() {
        return Err(anyhow!("В '{}' нет звуковых дескрипторов", path));
    }

    let mut scene = Scene::new("ImportedSounds");
    for s in &file.sounds {
        let name = file.get_string(s.name_id);
        if name.is_empty() {
            log(format!("⚠️ Пропущен звук без имени (name_id={})", s.name_id));
            continue;
        }
        scene.add_object(GameObject::new(
            name,
            ObjectType::AudioSource(AudioSourceComponent {
                sound_name: name.to_string(),
                volume: s.default_volume,
                spatial_blend: s.spatial_blend,
                enabled: true,
            }),
        ));
    }
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{AudioSourceComponent, GameObject};

    #[test]
    fn export_then_load_preserves_sound_descriptor() {
        let mut scene = Scene::new("TestAudio");
        scene.add_object(GameObject::new(
            "Siren",
            ObjectType::AudioSource(AudioSourceComponent {
                sound_name: "siren.wav".to_string(),
                volume: 0.75,
                spatial_blend: 1.0,
                enabled: true,
            }),
        ));

        let path = std::env::temp_dir().join("alkash3d_editor_alsnd_test.alsnd");
        let path_str = path.to_string_lossy().to_string();
        export_scene_to_alsnd_file(&scene, &path_str).expect("export");
        let loaded = AlsndFile::load(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(loaded.sounds.len(), 1);
        assert!((loaded.sounds[0].default_volume - 0.75).abs() < 1e-5);
        assert!((loaded.sounds[0].spatial_blend - 1.0).abs() < 1e-5);
        assert_eq!(loaded.banks.len(), 1);
    }

    #[test]
    fn import_recreates_audio_source_objects() {
        let mut scene = Scene::new("TestAudio");
        scene.add_object(GameObject::new(
            "Siren",
            ObjectType::AudioSource(AudioSourceComponent {
                sound_name: "siren.wav".to_string(),
                volume: 0.75,
                spatial_blend: 1.0,
                enabled: true,
            }),
        ));

        let path = std::env::temp_dir().join("alkash3d_editor_alsnd_import_test.alsnd");
        let path_str = path.to_string_lossy().to_string();
        export_scene_to_alsnd_file(&scene, &path_str).expect("export");

        let mut logs = Vec::new();
        let imported = import_alsnd_to_scene(&path_str, &mut |m| logs.push(m)).expect("import");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(imported.objects.len(), 1, "logs: {:?}", logs);
        let obj = imported.objects.values().next().unwrap();
        let ObjectType::AudioSource(a) = &obj.object_type else { panic!("expected AudioSource") };
        assert_eq!(a.sound_name, "siren.wav");
        assert!((a.volume - 0.75).abs() < 1e-5);
    }

    #[test]
    fn sound_bank_editor_round_trip() {
        let entries = vec![
            SoundEntryEdit { name: "engine_idle.wav".to_string(), category: 0, volume: 0.6, ..Default::default() },
            SoundEntryEdit { name: "  ".to_string(), ..Default::default() }, // пустая заготовка — должна отсеяться
            SoundEntryEdit { name: "horn.ogg".to_string(), format: 1, category: 4, priority: 200, max_instances: 1, spatial_blend: 0.2, ..Default::default() },
        ];

        let path = std::env::temp_dir().join("alkash3d_editor_sound_bank_editor_test.alsnd");
        let path_str = path.to_string_lossy().to_string();
        let count = save_sound_bank("CarSounds", &entries, &path_str).expect("save");
        assert_eq!(count, 2, "пустая запись должна была отсеяться");

        let (bank_name, loaded) = load_sound_bank(&path_str).expect("load");
        let _ = std::fs::remove_file(&path_str);

        assert_eq!(bank_name, "CarSounds");
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].name, "engine_idle.wav");
        assert_eq!(loaded[1].name, "horn.ogg");
        assert_eq!(loaded[1].format, 1);
        assert_eq!(loaded[1].priority, 200);
        assert!((loaded[1].spatial_blend - 0.2).abs() < 1e-5);
    }
}
