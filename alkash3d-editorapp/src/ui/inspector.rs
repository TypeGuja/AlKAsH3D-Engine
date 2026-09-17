use egui::*;
use uuid::Uuid;

use crate::math::Vec3;
use crate::scene::{AudioSourceComponent, LightComponent, LightType, ObjectType, ScriptedEntityComponent};

pub fn render_inspector(ctx: &egui::Context, app: &mut crate::EditorApp) {
    if !app.show_inspector { return; }

    egui::SidePanel::right("inspector")
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.heading("🔧 Inspector");
            ui.separator();

            let selected_ids = app.scene.selected_ids.clone();
            match selected_ids.len() {
                0 => {
                    ui.label("No object selected");
                }
                1 => render_single(ui, app, selected_ids[0]),
                _ => render_multi(ui, app, &selected_ids),
            }
        });
}

fn render_single(ui: &mut Ui, app: &mut crate::EditorApp, id: Uuid) {
    let Some(obj) = app.scene.get_object(id).cloned() else { return; };
    let mut o = obj;

    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.text_edit_singleline(&mut o.name);
    });

    ui.horizontal(|ui| {
        ui.label("Parent:");
        match o.parent.and_then(|p| app.scene.get_object(p)) {
            Some(parent) => {
                ui.label(&parent.name);
                if ui.small_button("✖").on_hover_text("Un-parent (move to Scene Root)").clicked() {
                    let _ = app.scene.set_parent(id, None);
                }
            }
            None => {
                ui.weak("— (root)");
            }
        }
    });

    ui.checkbox(&mut o.visible, "Visible");
    ui.checkbox(&mut o.locked, "Locked");

    ui.collapsing("Transform", |ui| {
        ui.label(RichText::new("Position").strong());
        ui.horizontal(|ui| {
            ui.label("X"); ui.add(egui::DragValue::new(&mut o.transform.position.x).speed(0.1));
            ui.label("Y"); ui.add(egui::DragValue::new(&mut o.transform.position.y).speed(0.1));
            ui.label("Z"); ui.add(egui::DragValue::new(&mut o.transform.position.z).speed(0.1));
        });

        ui.label(RichText::new("Rotation (°)").strong());
        let mut euler_deg = o.transform.rotation.to_euler() * (180.0 / std::f32::consts::PI);
        let before = euler_deg;
        ui.horizontal(|ui| {
            ui.label("X"); ui.add(egui::DragValue::new(&mut euler_deg.x).speed(1.0));
            ui.label("Y"); ui.add(egui::DragValue::new(&mut euler_deg.y).speed(1.0));
            ui.label("Z"); ui.add(egui::DragValue::new(&mut euler_deg.z).speed(1.0));
        });
        if euler_deg != before {
            let rad = euler_deg * (std::f32::consts::PI / 180.0);
            o.transform.rotation = crate::math::Quat::from_euler(rad.x, rad.y, rad.z);
        }

        ui.label(RichText::new("Scale").strong());
        ui.horizontal(|ui| {
            ui.label("X"); ui.add(egui::DragValue::new(&mut o.transform.scale.x).speed(0.01).range(0.01..=1000.0));
            ui.label("Y"); ui.add(egui::DragValue::new(&mut o.transform.scale.y).speed(0.01).range(0.01..=1000.0));
            ui.label("Z"); ui.add(egui::DragValue::new(&mut o.transform.scale.z).speed(0.01).range(0.01..=1000.0));
        });
    });

    ui.separator();

    // ДОБАВЛЕНО (по прямому запросу пользователя: "использовать форматы на
    // объектах в эдиторе") — Mesh/AudioSource ниже могут брать
    // геометрию/материал/звук напрямую из родных файлов движка (.altex/
    // .almat/.alsnd), а не только через отдельные, не привязанные к
    // конкретному объекту окна-редакторы (Material Library Editor/Sound
    // Bank Editor). `geometry_reload` откладывает пересоздание GPU-меша до
    // момента, когда изменённый `o` уже записан обратно в `app.scene` (см.
    // `*orig = o;` ниже) — `refresh_gpu_mesh_structural` читает геометрию
    // из `app.scene`, так что вызывать его раньше означало бы загрузить на
    // GPU ещё старый, не заменённый меш.
    let mut geometry_reload = false;

    match &mut o.object_type {
        ObjectType::Mesh(m) => {
            ui.collapsing("Mesh", |ui| {
                ui.label(format!("Vertices: {}  Indices: {}", m.mesh.vertices.len(), m.mesh.indices.len()));
                ui.checkbox(&mut m.wireframe, "Wireframe");
                ui.checkbox(&mut m.solid, "Solid");
                ui.checkbox(&mut m.double_sided, "Double-sided");
                if ui.small_button("📂 Load geometry from .altex...")
                    .on_hover_text("Заменить геометрию данными родного формата движка (материал не трогает)")
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("AlKAsH3D Model", &["altex"])
                        .pick_file()
                    {
                        let path_str = path.to_string_lossy().to_string();
                        match crate::converters::altex::import_altex(&path_str) {
                            Ok(mut meshes) if !meshes.is_empty() => {
                                let extra = meshes.len() - 1;
                                let (mesh_name, mesh, _material) = meshes.remove(0);
                                m.mesh = mesh;
                                geometry_reload = true;
                                if extra > 0 {
                                    app.log(&format!("✅ Геометрия загружена из '{}' (меш '{}'); ещё {} меш(ей) в файле проигнорировано", path_str, mesh_name, extra), Color32::GREEN);
                                } else {
                                    app.log(&format!("✅ Геометрия загружена из '{}' (меш '{}')", path_str, mesh_name), Color32::GREEN);
                                }
                            }
                            Ok(_) => app.log(&format!("⚠️ '{}' не содержит мешей", path_str), Color32::YELLOW),
                            Err(e) => app.log(&format!("❌ Ошибка загрузки .altex: {}", e), Color32::RED),
                        }
                    }
                }
            });
            ui.collapsing("Material", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut m.material.name);
                });
                ui.horizontal(|ui| {
                    ui.label("Library:");
                    let current = if m.material.name.is_empty() { "(custom)".to_string() } else { m.material.name.clone() };
                    egui::ComboBox::from_id_salt("mesh_material_library_combo")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            let mut names: Vec<String> = app.asset_library.materials.keys().cloned().collect();
                            names.sort();
                            for name in names {
                                if ui.selectable_label(m.material.name == name, &name).clicked() {
                                    if let Some(mat) = app.asset_library.materials.get(&name) {
                                        m.material = mat.clone();
                                    }
                                }
                            }
                        });
                    if ui.small_button("📂 Load .almat...")
                        .on_hover_text("Импортировать материалы движка в библиотеку и назначить первый объекту")
                        .clicked()
                    {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("AlKAsH3D Materials", &["almat"])
                            .pick_file()
                        {
                            let path_str = path.to_string_lossy().to_string();
                            let mut messages = Vec::new();
                            let result = crate::converters::almat::import_almat_to_materials(&path_str, &mut |msg| messages.push(msg));
                            for msg in messages {
                                app.log(&msg, Color32::YELLOW);
                            }
                            match result {
                                Ok(materials) if !materials.is_empty() => {
                                    let mut names: Vec<String> = materials.keys().cloned().collect();
                                    names.sort();
                                    let picked_name = names[0].clone();
                                    let extra = materials.len() - 1;
                                    app.asset_library.materials.extend(materials);
                                    if let Some(mat) = app.asset_library.materials.get(&picked_name) {
                                        m.material = mat.clone();
                                    }
                                    if extra > 0 {
                                        app.log(&format!("✅ Материал '{}' назначен объекту (ещё {} добавлено в библиотеку)", picked_name, extra), Color32::GREEN);
                                    } else {
                                        app.log(&format!("✅ Материал '{}' назначен объекту", picked_name), Color32::GREEN);
                                    }
                                }
                                Ok(_) => app.log(&format!("⚠️ '{}' не содержит материалов", path_str), Color32::YELLOW),
                                Err(e) => app.log(&format!("❌ Ошибка импорта .almat: {}", e), Color32::RED),
                            }
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Albedo:");
                    let mut rgba = m.material.color;
                    ui.color_edit_button_rgba_unmultiplied(&mut rgba);
                    m.material.color = rgba;
                });
                ui.add(egui::Slider::new(&mut m.material.metallic, 0.0..=1.0).text("Metallic"));
                ui.add(egui::Slider::new(&mut m.material.roughness, 0.0..=1.0).text("Roughness"));
                ui.horizontal(|ui| {
                    ui.label("Emissive:");
                    let mut e = m.material.emissive;
                    ui.color_edit_button_rgb(&mut e);
                    m.material.emissive = e;
                });
            });
        }
        ObjectType::Light(l) => {
            ui.collapsing("Light", |ui| {
                render_light_fields(ui, app, l);
            });
        }
        ObjectType::AudioSource(a) => {
            ui.collapsing("Audio Source", |ui| {
                render_audio_fields(ui, app, a);
            });
        }
        ObjectType::ScriptedEntity(s) => {
            ui.collapsing("Scripted Entity", |ui| {
                render_script_fields(ui, app, s);
            });
        }
        ObjectType::Camera(c) => {
            ui.collapsing("Camera", |ui| {
                ui.add(egui::Slider::new(&mut c.fov, 10.0..=170.0).text("FOV"));
                ui.add(egui::DragValue::new(&mut c.near).speed(0.01).range(0.001..=c.far));
                ui.add(egui::DragValue::new(&mut c.far).speed(1.0).range(c.near..=100000.0));
                ui.checkbox(&mut c.orthographic, "Orthographic");
            });
        }
        ObjectType::ParticleSystem(p) => {
            ui.collapsing("Particle System", |ui| {
                ui.checkbox(&mut p.enabled, "Enabled");
                ui.checkbox(&mut p.system.looping, "Looping");
                ui.add(egui::DragValue::new(&mut p.system.emission_rate).speed(0.5).range(0.0..=1000.0).prefix("Emission Rate: "));
                ui.add(egui::DragValue::new(&mut p.system.max_particles).speed(1.0).range(1..=100000).prefix("Max Particles: "));
                ui.add(egui::Slider::new(&mut p.system.lifetime, 0.1..=20.0).text("Lifetime (s)"));
                ui.add(egui::DragValue::new(&mut p.system.start_size).speed(0.01).range(0.0..=100.0).prefix("Start Size: "));
                ui.add(egui::DragValue::new(&mut p.system.end_size).speed(0.01).range(0.0..=100.0).prefix("End Size: "));
                ui.horizontal(|ui| {
                    ui.label("Start Color:");
                    let mut c = p.system.start_color;
                    ui.color_edit_button_rgba_unmultiplied(&mut c);
                    p.system.start_color = c;
                });
                ui.horizontal(|ui| {
                    ui.label("End Color:");
                    let mut c = p.system.end_color;
                    ui.color_edit_button_rgba_unmultiplied(&mut c);
                    p.system.end_color = c;
                });
                ui.horizontal(|ui| {
                    ui.label("Velocity:");
                    ui.add(egui::DragValue::new(&mut p.system.velocity.x).speed(0.1).prefix("X: "));
                    ui.add(egui::DragValue::new(&mut p.system.velocity.y).speed(0.1).prefix("Y: "));
                    ui.add(egui::DragValue::new(&mut p.system.velocity.z).speed(0.1).prefix("Z: "));
                });
                ui.add(egui::DragValue::new(&mut p.system.velocity_random).speed(0.05).range(0.0..=50.0).prefix("Velocity Random: "));
                ui.horizontal(|ui| {
                    ui.label("Gravity:");
                    ui.add(egui::DragValue::new(&mut p.system.gravity.x).speed(0.1).prefix("X: "));
                    ui.add(egui::DragValue::new(&mut p.system.gravity.y).speed(0.1).prefix("Y: "));
                    ui.add(egui::DragValue::new(&mut p.system.gravity.z).speed(0.1).prefix("Z: "));
                });
                ui.weak(format!("Живых частиц: {}", p.system.particles.len()));
            });
        }
        ObjectType::SpawnPoint => {
            ui.collapsing("Spawn Point", |ui| {
                ui.weak("Здесь появляется игрок при загрузке мира. Позиция — Transform > Position выше; направление взгляда — Transform > Rotation (Y = поворот по горизонтали).");
                let facing = o.transform.rotation.forward();
                ui.label(format!("Facing: ({:.2}, {:.2}, {:.2})", facing.x, facing.y, facing.z));
                let spawn_count = app.scene.objects.values().filter(|obj| matches!(obj.object_type, ObjectType::SpawnPoint)).count();
                if spawn_count > 1 {
                    ui.colored_label(Color32::YELLOW, format!("⚠️ В сцене {} точек спавна — движок использует только одну (первую найденную) при экспорте в .alworld.", spawn_count));
                }
            });
        }
        ObjectType::Empty => {
            ui.weak("Empty object — group/anchor only, no rendering.");
        }
    }

    render_animation_fields(ui, &mut o);

    if let Some(orig) = app.scene.get_object_mut(id) {
        *orig = o;
    }
    if geometry_reload {
        app.refresh_gpu_mesh_structural(id);
    }

    ui.separator();
    if ui.button("🗑 Remove").clicked() {
        if app.gpu_renderer.is_some() {
            app.gpu_mesh_map.remove(&id);
            app.gpu_material_map.remove(&id);
        }
        app.scene.remove_object(id);
    }
}

fn render_light_fields(ui: &mut Ui, app: &mut crate::EditorApp, l: &mut LightComponent) {
    ui.checkbox(&mut l.enabled, "Enabled");

    if ui.small_button("📂 Load from .alfar...")
        .on_hover_text("Назначить параметры из сохранённого набора освещения (.alfar) — родной формат движка")
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Lighting", &["alfar"])
            .pick_file()
        {
            let path_str = path.to_string_lossy().to_string();
            match crate::converters::alfar::load_lights_for_picker(&path_str) {
                Ok(entries) if !entries.is_empty() => {
                    let extra = entries.len() - 1;
                    let (picked_name, picked) = entries.into_iter().next().unwrap();
                    let needs_rotation_fix = !matches!(picked.light_type, LightType::Point);
                    *l = picked;
                    if extra > 0 {
                        app.log(&format!("✅ Свет '{}' назначен из '{}' (ещё {} в файле проигнорировано)", picked_name, path_str, extra), Color32::GREEN);
                    } else {
                        app.log(&format!("✅ Свет '{}' назначен из '{}'", picked_name, path_str), Color32::GREEN);
                    }
                    if needs_rotation_fix {
                        app.log("⚠️ Направление Spot/Directional света .alfar не хранит поворот объекта — поправьте Rotation в Transform вручную", Color32::YELLOW);
                    }
                }
                Ok(_) => app.log(&format!("⚠️ '{}' не содержит источников света", path_str), Color32::YELLOW),
                Err(e) => app.log(&format!("❌ Ошибка загрузки .alfar: {}", e), Color32::RED),
            }
        }
    }

    // ДОБАВЛЕНО (по прямому запросу пользователя: "id света чтобы каждый
    // раз на свет не загружать almat для нужного оттенка") — переиспользуемый
    // оттенок по числовому ID (см. assets/groups.rs::LightColorGroup):
    // назначить уже существующий цвет/яркость другому светильнику можно из
    // выпадающего списка ниже, без диалога открытия файла каждый раз, а
    // правка самой группы применяется сразу ко всем светильникам с этим ID.
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Color Group:");
        let current_label = l.color_group
            .and_then(|gid| app.light_color_groups.get(&gid))
            .map(|g| g.name.clone())
            .unwrap_or_else(|| "(none)".to_string());
        egui::ComboBox::from_id_salt("light_color_group_combo")
            .selected_text(current_label)
            .show_ui(ui, |ui| {
                if ui.selectable_label(l.color_group.is_none(), "(none)").clicked() {
                    l.color_group = None;
                }
                let mut ids: Vec<u32> = app.light_color_groups.keys().copied().collect();
                ids.sort();
                for gid in ids {
                    let name = app.light_color_groups[&gid].name.clone();
                    if ui.selectable_label(l.color_group == Some(gid), &name).clicked() {
                        let g = app.light_color_groups[&gid].clone();
                        l.color_group = Some(gid);
                        l.color = g.color;
                        l.intensity = g.intensity;
                    }
                }
            });
        if ui.small_button("➕ Save as group")
            .on_hover_text("Запомнить текущий цвет/яркость под новым ID для переиспользования на других светильниках")
            .clicked()
        {
            let gid = app.next_light_color_group_id;
            app.next_light_color_group_id += 1;
            app.light_color_groups.insert(gid, crate::assets::LightColorGroup {
                name: format!("Shade {}", gid),
                color: l.color,
                intensity: l.intensity,
            });
            l.color_group = Some(gid);
            app.save_asset_groups();
        }
    });
    if let Some(gid) = l.color_group {
        if let Some(group) = app.light_color_groups.get(&gid).cloned() {
            let mut name = group.name.clone();
            let mut color = group.color;
            let mut intensity = group.intensity;
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label("Group name:");
                if ui.text_edit_singleline(&mut name).changed() { changed = true; }
            });
            ui.horizontal(|ui| {
                ui.label("Group color:");
                if ui.color_edit_button_rgb(&mut color).changed() { changed = true; }
                ui.label("Intensity:");
                if ui.add(egui::Slider::new(&mut intensity, 0.0..=20.0)).changed() { changed = true; }
            });
            if changed {
                if let Some(g) = app.light_color_groups.get_mut(&gid) {
                    g.name = name;
                    g.color = color;
                    g.intensity = intensity;
                }
                app.sync_light_color_group(gid);
                app.save_asset_groups();
            }
            ui.weak("Правка группы применяется сразу ко всем светильникам с этим оттенком.");
        }
    }
    ui.separator();

    let mut kind = match l.light_type {
        LightType::Point => 0,
        LightType::Directional => 1,
        LightType::Spot { .. } => 2,
    };
    egui::ComboBox::from_label("Type")
        .selected_text(match kind { 0 => "Point", 1 => "Directional", _ => "Spot" })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut kind, 0, "Point");
            ui.selectable_value(&mut kind, 1, "Directional");
            ui.selectable_value(&mut kind, 2, "Spot");
        });
    l.light_type = match (kind, &l.light_type) {
        (0, _) => LightType::Point,
        (1, _) => LightType::Directional,
        (2, LightType::Spot { inner_angle, outer_angle }) => LightType::Spot { inner_angle: *inner_angle, outer_angle: *outer_angle },
        (2, _) => LightType::Spot { inner_angle: 25.0, outer_angle: 35.0 },
        _ => l.light_type.clone(),
    };
    if let LightType::Spot { inner_angle, outer_angle } = &mut l.light_type {
        ui.add(egui::Slider::new(inner_angle, 0.0..=89.0).text("Inner Angle"));
        ui.add(egui::Slider::new(outer_angle, 0.0..=90.0).text("Outer Angle"));
    }

    ui.horizontal(|ui| {
        ui.label("Color:");
        ui.color_edit_button_rgb(&mut l.color);
    });
    ui.add(egui::Slider::new(&mut l.intensity, 0.0..=20.0).text("Intensity"));
    ui.add(egui::Slider::new(&mut l.range, 0.0..=500.0).text("Range"));
}

fn render_audio_fields(ui: &mut Ui, app: &mut crate::EditorApp, a: &mut AudioSourceComponent) {
    ui.checkbox(&mut a.enabled, "Enabled");
    ui.horizontal(|ui| {
        ui.label("Sound name:");
        ui.text_edit_singleline(&mut a.sound_name);
        if ui.small_button("📂").on_hover_text("Browse for a sound file (.wav/.ogg/.mp3/.flac/.opus)").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Audio", &["wav", "ogg", "mp3", "flac", "opus"])
                .pick_file()
            {
                a.sound_name = path.to_string_lossy().to_string();
            }
        }
    });
    if ui.small_button("📂 Load from .alsnd bank...")
        .on_hover_text("Назначить звук из сохранённого банка (.alsnd) — родной формат движка")
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Sound Bank", &["alsnd"])
            .pick_file()
        {
            let path_str = path.to_string_lossy().to_string();
            match crate::converters::alsnd::load_sound_bank(&path_str) {
                Ok((_bank_name, entries)) if !entries.is_empty() => {
                    let extra = entries.len() - 1;
                    let picked = entries.into_iter().next().unwrap();
                    a.sound_name = picked.name.clone();
                    a.volume = picked.volume;
                    a.spatial_blend = picked.spatial_blend;
                    if extra > 0 {
                        app.log(&format!("✅ Звук '{}' назначен из '{}' (ещё {} в банке проигнорировано)", picked.name, path_str, extra), Color32::GREEN);
                    } else {
                        app.log(&format!("✅ Звук '{}' назначен из '{}'", picked.name, path_str), Color32::GREEN);
                    }
                }
                Ok(_) => app.log(&format!("⚠️ '{}' не содержит звуков", path_str), Color32::YELLOW),
                Err(e) => app.log(&format!("❌ Ошибка загрузки .alsnd: {}", e), Color32::RED),
            }
        }
    }
    ui.add(egui::Slider::new(&mut a.volume, 0.0..=1.0).text("Volume"));
    ui.add(egui::Slider::new(&mut a.spatial_blend, 0.0..=1.0).text("Spatial Blend (0=2D, 1=3D)"));
}

fn render_script_fields(ui: &mut Ui, app: &mut crate::EditorApp, s: &mut ScriptedEntityComponent) {
    ui.checkbox(&mut s.enabled, "Enabled");
    ui.horizontal(|ui| {
        ui.label("Script:");
        ui.text_edit_singleline(&mut s.script_name);
        if ui.small_button("📂").on_hover_text("Browse for .py/.dll/.lua").clicked() {
            if let Some(path) = rfd::FileDialog::new()
                .add_filter("Scripts", &["py", "dll", "lua"])
                .pick_file()
            {
                s.script_name = path.to_string_lossy().to_string();
            }
        }
    });
    if ui.small_button("📂 Load from .alscript registry...")
        .on_hover_text("Назначить скрипт из сохранённого реестра (.alscript) — родной формат движка")
        .clicked()
    {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("AlKAsH3D Scripts", &["alscript"])
            .pick_file()
        {
            let path_str = path.to_string_lossy().to_string();
            let mut messages = Vec::new();
            let result = crate::converters::alscript::load_scripts(&path_str, &mut |msg| messages.push(msg));
            for msg in messages {
                app.log(&msg, Color32::YELLOW);
            }
            match result {
                Ok(entries) if !entries.is_empty() => {
                    let extra = entries.len() - 1;
                    let picked = entries.into_iter().next().unwrap();
                    s.script_name = picked.path.clone();
                    if extra > 0 {
                        app.log(&format!("✅ Скрипт '{}' ('{}') назначен из '{}' (ещё {} в реестре проигнорировано)", picked.name, picked.path, path_str, extra), Color32::GREEN);
                    } else {
                        app.log(&format!("✅ Скрипт '{}' ('{}') назначен из '{}'", picked.name, picked.path, path_str), Color32::GREEN);
                    }
                }
                Ok(_) => app.log(&format!("⚠️ '{}' не содержит скриптов", path_str), Color32::YELLOW),
                Err(e) => app.log(&format!("❌ Ошибка загрузки .alscript: {}", e), Color32::RED),
            }
        }
    }
    ui.weak("Тип скрипта (Python/Native/Lua) определяется по расширению при экспорте в .alscript.");
}

/// Мультивыделение: поля Position/Rotation/Scale работают как "приращение
/// с этого кадра" — `delta` создаётся заново на КАЖДОМ кадре (Vec3::ZERO),
/// `DragValue` во время перетаскивания добавляет к нему изменение мыши ЭТОГО
/// кадра (собственное состояние перетаскивания egui хранит отдельно, не в
/// самом значении) — то есть каждый кадр непрерывного перетаскивания
/// вносит свою маленькую поправку сразу во ВСЕ выделенные объекты, и сумма
/// поправок за весь жест перетаскивания равна общему смещению мыши. Не
/// пытаемся показать "общее" значение (оно бы отличалось у разных
/// объектов) — только явно подписанные поля "Move by/Rotate by/Scale by".
fn render_multi(ui: &mut Ui, app: &mut crate::EditorApp, ids: &[Uuid]) {
    ui.label(format!("{} objects selected", ids.len()));
    ui.separator();

    ui.collapsing("Transform (apply to all)", |ui| {
        let mut move_by = Vec3::ZERO;
        ui.label("Move by:");
        ui.horizontal(|ui| {
            ui.label("X"); ui.add(egui::DragValue::new(&mut move_by.x).speed(0.1));
            ui.label("Y"); ui.add(egui::DragValue::new(&mut move_by.y).speed(0.1));
            ui.label("Z"); ui.add(egui::DragValue::new(&mut move_by.z).speed(0.1));
        });

        let mut rotate_by_deg = Vec3::ZERO;
        ui.label("Rotate by (°):");
        ui.horizontal(|ui| {
            ui.label("X"); ui.add(egui::DragValue::new(&mut rotate_by_deg.x).speed(1.0));
            ui.label("Y"); ui.add(egui::DragValue::new(&mut rotate_by_deg.y).speed(1.0));
            ui.label("Z"); ui.add(egui::DragValue::new(&mut rotate_by_deg.z).speed(1.0));
        });

        let mut scale_by = Vec3::ZERO;
        ui.label("Scale by:");
        ui.horizontal(|ui| {
            ui.label("X"); ui.add(egui::DragValue::new(&mut scale_by.x).speed(0.01));
            ui.label("Y"); ui.add(egui::DragValue::new(&mut scale_by.y).speed(0.01));
            ui.label("Z"); ui.add(egui::DragValue::new(&mut scale_by.z).speed(0.01));
        });

        if move_by != Vec3::ZERO || rotate_by_deg != Vec3::ZERO || scale_by != Vec3::ZERO {
            let rotate_rad = rotate_by_deg * (std::f32::consts::PI / 180.0);
            let delta_rot = crate::math::Quat::from_euler(rotate_rad.x, rotate_rad.y, rotate_rad.z);
            for &id in ids {
                if let Some(obj) = app.scene.get_object_mut(id) {
                    obj.transform.position = obj.transform.position + move_by;
                    if rotate_by_deg != Vec3::ZERO {
                        obj.transform.rotation = obj.transform.rotation.mul(&delta_rot);
                    }
                    obj.transform.scale = Vec3::new(
                        (obj.transform.scale.x + scale_by.x).max(0.01),
                        (obj.transform.scale.y + scale_by.y).max(0.01),
                        (obj.transform.scale.z + scale_by.z).max(0.01),
                    );
                }
            }
        }
    });

    ui.separator();
    ui.label("Objects:");
    egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
        for &id in ids {
            if let Some(obj) = app.scene.get_object(id) {
                ui.label(&obj.name);
            }
        }
    });

    ui.separator();
    if ui.button(format!("🗑 Remove {} objects", ids.len())).clicked() {
        for &id in ids {
            if app.gpu_renderer.is_some() {
                app.gpu_mesh_map.remove(&id);
                app.gpu_material_map.remove(&id);
            }
            app.scene.remove_object(id);
        }
    }
}

/// ДОБАВЛЕНО (проигрывание анимаций — см. `Scene::update` про сам
/// плейбэк): раньше `GameObject.animations` можно было только хранить в
/// коде (клиппинг Track/Keyframe был написан, но нигде не вызывался и
/// авторить keyframe'ы было решительно нечем) — секция ниже даёт простой
/// цикл записи пути (по прямому запросу пользователя): создать анимацию →
/// "📍 Поставить точку здесь" (фиксирует текущий `o.transform` как точку в
/// конце пути, Time трогать не нужно) → подвинуть объект (гизмо/Transform-
/// секцией) → снова "📍 Поставить точку здесь" для следующей точки, и т.д.
/// "🕓 Записать позу на выбранный Time" — для точечной правки уже
/// существующей точки после перехода к ней кнопкой ⏵ в списке Keyframes.
/// Универсальная секция — не привязана к конкретному ObjectType, т.к.
/// `animations` живёт на самом `GameObject`.
fn render_animation_fields(ui: &mut Ui, o: &mut crate::scene::GameObject) {
    ui.separator();
    ui.collapsing("🎬 Animations", |ui| {
        if ui.small_button("+ Add Animation").clicked() {
            let name = format!("Animation {}", o.animations.len() + 1);
            o.animations.insert(name.clone(), crate::scene::game_object::Animation::new(name));
        }

        let mut names: Vec<String> = o.animations.keys().cloned().collect();
        names.sort();
        let mut to_remove: Option<String> = None;

        for name in names {
            let Some(anim) = o.animations.get_mut(&name) else { continue; };
            ui.push_id(&name, |ui| {
                ui.collapsing(&name, |ui| {
                    ui.horizontal(|ui| {
                        let label = if anim.playing { "⏸ Stop" } else { "▶ Play" };
                        if ui.button(label).clicked() {
                            anim.playing = !anim.playing;
                        }
                        if ui.button("🗑").on_hover_text("Удалить анимацию").clicked() {
                            to_remove = Some(name.clone());
                        }
                    });
                    ui.checkbox(&mut anim.show_keyframes, "Показывать точки во вьюпорте (можно двигать мышью)");
                    // Минимум 0.1, не 0.0 — см. комментарий у `Animation::new`
                    // про то, как duration=0 запирал Time-слайдер и делал
                    // невозможным второй отдельный keyframe.
                    ui.add(egui::DragValue::new(&mut anim.duration).speed(0.1).range(0.1..=600.0).prefix("Duration: "));
                    let max_t = anim.duration.max(0.001);
                    if anim.current_time > max_t { anim.current_time = max_t; }
                    // ИСПРАВЛЕНО (баг: "точка перемещается вместе с
                    // фигурой" / фигуру нельзя было отодвинуть от точки) —
                    // `time_before_frame` запоминается здесь, ДО слайдера и
                    // кнопок ⏵ в списке Keyframes ниже, которые единственные
                    // меняют `current_time`; скраб-превью в конце функции
                    // срабатывает, только если `current_time` РЕАЛЬНО
                    // изменился в этом кадре. Раньше превью применялось
                    // БЕЗУСЛОВНО каждый UI-кадр, пока анимация не играет —
                    // то есть каждый кадр молча перезаписывало
                    // `o.transform.position` обратно на позицию keyframe'а,
                    // отменяя любое перемещение объекта гизмо в предыдущем
                    // кадре (гизмо во вьюпорте рисуется ПОСЛЕ инспектора в
                    // том же UI-кадре — см. порядок вызовов в update()).
                    let time_before_frame = anim.current_time;
                    ui.add(egui::Slider::new(&mut anim.current_time, 0.0..=max_t).text("Time"));

                    // ДОБАВЛЕНО (по прямому запросу пользователя: "поставить
                    // 1 точку ... переместить фигуру в другое место и
                    // поставить 2 точку") — основной, самый простой способ
                    // авторить путь: жмём, двигаем объект гизмо/Transform-
                    // секцией, снова жмём. Time НЕ нужно трогать вручную —
                    // каждая точка сама встаёт на секунду позже последней
                    // (Duration при необходимости растёт следом), получая
                    // автоимя "Point N".
                    if ui.button("📍 Поставить точку здесь").on_hover_text("Записывает текущую позу объекта как НОВУЮ точку в конце пути — подвиньте объект и нажмите снова для следующей").clicked() {
                        let last_time = anim.position_track.keyframes.iter().map(|k| k.time).fold(-1.0_f32, f32::max);
                        let t = if last_time < 0.0 { 0.0 } else { last_time + 1.0 };
                        if anim.duration < t + 0.5 { anim.duration = t + 0.5; }
                        anim.current_time = t;
                        let point_name = format!("Point {}", anim.position_track.keyframes.len() + 1);
                        anim.position_track.add_keyframe(t, o.transform.position, crate::animation::EasingType::Linear, point_name.clone());
                        anim.rotation_track.add_keyframe(t, o.transform.rotation, crate::animation::EasingType::Linear, point_name.clone());
                        anim.scale_track.add_keyframe(t, o.transform.scale, crate::animation::EasingType::Linear, point_name);
                    }
                    if ui.small_button("🕓 Записать позу на выбранный Time").on_hover_text("Для точечной правки: перейдите к точке кнопкой ⏵ в списке ниже, поправьте позу, нажмите это — обновит именно её (имя сохранится)").clicked() {
                        let t = anim.current_time;
                        let existing_name = anim.position_track.keyframes.iter()
                            .find(|k| (k.time - t).abs() < 1e-4)
                            .map(|k| k.name.clone());
                        let name = existing_name.unwrap_or_else(|| format!("Point {}", anim.position_track.keyframes.len() + 1));
                        anim.position_track.add_keyframe(t, o.transform.position, crate::animation::EasingType::Linear, name.clone());
                        anim.rotation_track.add_keyframe(t, o.transform.rotation, crate::animation::EasingType::Linear, name.clone());
                        anim.scale_track.add_keyframe(t, o.transform.scale, crate::animation::EasingType::Linear, name);
                    }
                    ui.collapsing(format!(
                        "Keyframes ({} / {} / {})",
                        anim.position_track.keyframes.len(),
                        anim.rotation_track.keyframes.len(),
                        anim.scale_track.keyframes.len(),
                    ), |ui| {
                        render_keyframe_list(ui, "Position", &mut anim.position_track, &mut anim.current_time);
                        render_keyframe_list(ui, "Rotation", &mut anim.rotation_track, &mut anim.current_time);
                        render_keyframe_list(ui, "Scale", &mut anim.scale_track, &mut anim.current_time);
                    });

                    // Скраб-превью: срабатывает ТОЛЬКО в кадре, где сам
                    // пользователь подвинул Time (слайдер выше или ⏵ в
                    // списке Keyframes) — не каждый кадр (см. комментарий у
                    // `time_before_frame` выше про то, почему это раньше
                    // ломало ручное перемещение объекта). Пока анимация
                    // играет — превью не нужно, `Scene::update` и так
                    // применяет позу каждый кадр. `apply_to_transform`
                    // (а не `get_transform`) — треки без keyframes не
                    // трогают тот компонент transform'а вообще, иначе
                    // разворачивание пустой свежесозданной анимации сразу
                    // сбрасывало бы Position/Rotation/Scale объекта в 0/
                    // identity/1.
                    if !anim.playing && anim.current_time != time_before_frame {
                        anim.apply_to_transform(&mut o.transform);
                    }
                });
            });
        }

        if let Some(name) = to_remove {
            o.animations.remove(&name);
        }
    });
}

/// ДОБАВЛЕНО (по прямому запросу пользователя: "для KeyFrame отдельный
/// пункт который включает их отображение чтобы их можно было двигать", и
/// далее "название" над каждой точкой) — список keyframe'ов одного трека:
/// редактируемое имя (то же, что подписано над маркером во вьюпорте, см.
/// `EditorApp::draw_keyframe_markers`), редактируемое время (перетаскивание
/// = "двигать" keyframe вдоль таймлайна), кнопка перехода к нему (ставит
/// `current_time`, дальше скраб-превью в `render_animation_fields`
/// применяет позу этого момента — так можно поправить саму позу и нажать
/// "🕓 Записать позу на выбранный Time" в `render_animation_fields`, что
/// ЗАМЕНЯЕТ этот keyframe, а не добавляет новый, см. `AnimationTrack::
/// add_keyframe`) и удаление. Само значение (Vec3/Quat) здесь не
/// редактируется напрямую — через обычную Transform-секцию после перехода.
fn render_keyframe_list<T: Clone + crate::animation::Interpolatable>(
    ui: &mut Ui,
    label: &str,
    track: &mut crate::animation::AnimationTrack<T>,
    current_time: &mut f32,
) {
    if track.keyframes.is_empty() {
        ui.weak(format!("{}: нет keyframe'ов", label));
        return;
    }
    ui.label(label);
    let mut remove_idx: Option<usize> = None;
    let mut needs_resort = false;
    for (i, kf) in track.keyframes.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut kf.name).desired_width(70.0));
            let before = kf.time;
            ui.add(egui::DragValue::new(&mut kf.time).speed(0.05).range(0.0..=600.0).prefix("t="));
            if kf.time != before { needs_resort = true; }
            if ui.small_button("⏵").on_hover_text("Перейти к этому времени (для правки позы)").clicked() {
                *current_time = kf.time;
            }
            if ui.small_button("🗑").clicked() {
                remove_idx = Some(i);
            }
        });
    }
    if let Some(i) = remove_idx {
        track.keyframes.remove(i);
    }
    if needs_resort {
        track.keyframes.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());
    }
}
