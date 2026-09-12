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

    match &mut o.object_type {
        ObjectType::Mesh(m) => {
            ui.collapsing("Mesh", |ui| {
                ui.label(format!("Vertices: {}  Indices: {}", m.mesh.vertices.len(), m.mesh.indices.len()));
                ui.checkbox(&mut m.wireframe, "Wireframe");
                ui.checkbox(&mut m.solid, "Solid");
                ui.checkbox(&mut m.double_sided, "Double-sided");
            });
            ui.collapsing("Material", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Name:");
                    ui.text_edit_singleline(&mut m.material.name);
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
                render_light_fields(ui, l);
            });
        }
        ObjectType::AudioSource(a) => {
            ui.collapsing("Audio Source", |ui| {
                render_audio_fields(ui, a);
            });
        }
        ObjectType::ScriptedEntity(s) => {
            ui.collapsing("Scripted Entity", |ui| {
                render_script_fields(ui, s);
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

    if let Some(orig) = app.scene.get_object_mut(id) {
        *orig = o;
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

fn render_light_fields(ui: &mut Ui, l: &mut LightComponent) {
    ui.checkbox(&mut l.enabled, "Enabled");

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

fn render_audio_fields(ui: &mut Ui, a: &mut AudioSourceComponent) {
    ui.checkbox(&mut a.enabled, "Enabled");
    ui.horizontal(|ui| {
        ui.label("Sound name:");
        ui.text_edit_singleline(&mut a.sound_name);
    });
    ui.add(egui::Slider::new(&mut a.volume, 0.0..=1.0).text("Volume"));
    ui.add(egui::Slider::new(&mut a.spatial_blend, 0.0..=1.0).text("Spatial Blend (0=2D, 1=3D)"));
}

fn render_script_fields(ui: &mut Ui, s: &mut ScriptedEntityComponent) {
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
