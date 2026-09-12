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

use crate::scene::{ObjectType, Scene};

use alkash3d_rs::{AlsndFile, SoundBank, SoundDescriptor};

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
}
