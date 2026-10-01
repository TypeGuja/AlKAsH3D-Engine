//! Свет уличных фонарей карты -> .alfar для движка.
//!
//! samara_alfar <папка lights/> <выход.alfar> [--center X,Z] [--radius М] [--max N]
//!
//! Читает lights/chunk_<gx>_<gz>.csv (x,y,z,kind — центры рассеивателей
//! фонарей, их пишет tools/samara_map/build_chunks.py). По умолчанию берёт
//! ВСЕ фонари карты; --center/--radius/--max — чтобы урезать (ближайшие к
//! центру). Раньше по умолчанию был радиус 950 м: сетка каллинга FirstFires
//! стояла вокруг начала координат мира, а день/ночь искал каждый фонарь
//! линейно (N² за кадр). Теперь сетка едет за камерой, поиск — O(1), и
//! движок отправляет в плагин только фонари с изменившейся яркостью.
use std::path::Path;

use alkash3d_rs::{AlfarFile, AmbientLight, IndividualLight, LightFalloff};

/// `alfar_format::LightType::Spot` (сам enum из корня крейта не импортировать —
/// имя `LightType` там неоднозначно между модулями движка).
const LIGHT_TYPE_SPOT: u32 = 1;

struct Lamp {
    pos: [f32; 3],
    kind: String,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: samara_alfar <lights_dir> <out.alfar> [--center X,Z] [--radius M] [--max N]");
        std::process::exit(2);
    }
    let dir = Path::new(&args[1]);
    let out = &args[2];
    let mut center = [0.0f32, 0.0];
    let mut radius = f32::INFINITY;
    let mut max = usize::MAX;
    let mut i = 3;
    while i + 1 < args.len() {
        match args[i].as_str() {
            "--center" => {
                let v: Vec<f32> = args[i + 1].split(',').map(|s| s.trim().parse().expect("--center X,Z")).collect();
                center = [v[0], v[1]];
            }
            "--radius" => radius = args[i + 1].parse().expect("--radius"),
            "--max" => max = args[i + 1].parse().expect("--max"),
            other => panic!("неизвестный аргумент {}", other),
        }
        i += 2;
    }

    let mut lamps = Vec::new();
    for entry in std::fs::read_dir(dir).expect("папка lights/") {
        let path = entry.unwrap().path();
        if path.extension().map_or(true, |e| e != "csv") {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 4 {
                continue;
            }
            let pos: [f32; 3] = [f[0].parse().unwrap(), f[1].parse().unwrap(), f[2].parse().unwrap()];
            let d = ((pos[0] - center[0]).powi(2) + (pos[2] - center[1]).powi(2)).sqrt();
            if d <= radius {
                lamps.push((d, Lamp { pos, kind: f[3].to_string() }));
            }
        }
    }
    lamps.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let total_in_radius = lamps.len();
    lamps.truncate(max);

    let mut alfar = AlfarFile::new();
    // ночной город: слабый холодный ambient, основное — от фонарей
    alfar.ambient = AmbientLight { intensity: 0.05, color: [0.05, 0.05, 0.1], skybox_intensity: 0.1, use_skybox: 1 };
    alfar.global_settings.bloom_intensity = 0.8;
    alfar.global_settings.exposure = 0.8;

    let mut counts = std::collections::BTreeMap::new();
    for (n, (_, lamp)) in lamps.iter().enumerate() {
        // цвет/сила/конус: LED 4000K на магистралях, натрий (ДНаТ ~2000K) во
        // дворах и на жилых улицах, тёплый LED в парках.
        // Сила — под честное затухание 1/r² шейдера (main_ps.hlsl,
        // ComputePointLightContribution): светильник висит на ~9 м (парковый
        // ~5 м), и чтобы под ним было пятно ~0.3–0.4 (полуденное солнце в
        // движке = 1.0, ночной ambient ~0.03), нужно I ≈ 0.4 * 9² ≈ 32.
        //
        // "entrance" — светильник над подъездом (osm_details.py): ~10 Вт LED,
        // ~1000 лм с широким косинусным светом => пиковая сила ~1000/π ≈ 320 кд,
        // примерно в 19 раз слабее уличного LED-светильника (~6000 кд) — отсюда
        // 32 / 19 ≈ 1.7. Висит на 2.4 м, поэтому пятно у двери по яркости
        // сравнимо с пятном под уличным фонарём, но радиус ~10 м. 3000K.
        let (color, intensity, range, outer, inner) = match lamp.kind.as_str() {
            "led" => ([1.0, 0.89, 0.78], 32.0, 40.0, 1.15, 0.6),
            "sodium" => ([1.0, 0.62, 0.28], 26.0, 36.0, 1.2, 0.7),
            "entrance" => ([1.0, 0.82, 0.62], 1.7, 12.0, 1.4, 0.9),
            _ => ([1.0, 0.85, 0.65], 8.0, 20.0, 1.3, 0.8),
        };
        // ~2% натриевых ламп "моргают" — изношенный ДНаТ
        let hash = (lamp.pos[0] * 7.3) as i64 * 73856093 ^ (lamp.pos[2] * 7.3) as i64 * 19349663;
        let broken = lamp.kind == "sodium" && hash.rem_euclid(50) == 0;
        *counts.entry(lamp.kind.clone()).or_insert(0usize) += 1;
        let light = IndividualLight {
            id: 0,
            name_id: 0,
            light_type: LIGHT_TYPE_SPOT,
            position: lamp.pos,
            direction: [0.0, -1.0, 0.0],
            up: [1.0, 0.0, 0.0],
            color,
            intensity,
            range,
            falloff_type: LightFalloff::Quadratic as u32,
            falloff_custom: 0.0,
            spot_inner_angle: inner,
            spot_outer_angle: outer,
            // тысячи теней от фонарей движок не потянет — тени только от солнца/луны
            casts_shadows: 0,
            shadow_bias: 0.005,
            shadow_resolution: 512,
            flicker_enabled: broken as u32,
            flicker_speed: 7.0,
            flicker_intensity: 0.6,
            enabled: 1,
            active_from: 18.0,
            active_to: 6.0,
            has_physics: 0,
            breakable: 1,
            health: 100.0,
            custom_data_offset: 0,
        };
        let prefix = if lamp.kind == "entrance" { "EntranceLamp" } else { "StreetLamp" };
        alfar.add_light(light, &format!("{}_{}_{}", prefix, lamp.kind, n));
    }
    alfar.save(out).expect("не удалось записать .alfar");
    // контрольное чтение тем же загрузчиком, что у движка (load_lights_from_alfar)
    let back = AlfarFile::load(out).expect("записанный .alfar не читается движком");
    assert_eq!(back.lights.len(), lamps.len(), "число источников после чтения не совпало");
    if let (Some(a), Some(b)) = (back.lights.first(), lamps.first()) {
        assert_eq!(a.position, b.1.pos, "позиция первого фонаря после чтения не совпала");
    }
    println!(
        "{}: {} фонарей (в радиусе {:.0} м вокруг {:?} было {}), {:?}",
        out, lamps.len(), radius, center, total_in_radius, counts
    );
}
