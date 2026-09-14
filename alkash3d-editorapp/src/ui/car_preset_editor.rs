// src/ui/car_preset_editor.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя — см. `ui/sound_bank_editor.rs`
// про общий паттерн этой серии редакторов): полный редактор одного
// `.alcar`-пресета (метаданные + физика + число фонарей) — см.
// `converters::alcar::CarPresetEdit`. В отличие от `Assets > Car Preset`
// (готовые Default/Sports/Police пресеты в File > Export), здесь каждое
// поле настраивается вручную.

use egui::*;
use crate::EditorApp;

pub fn render_car_preset_editor(ctx: &egui::Context, app: &mut EditorApp) {
    if !app.car_preset_editor.open {
        return;
    }

    let mut still_open = true;
    let mut do_load = false;
    let mut do_save = false;

    egui::Window::new("🚗 Car Preset Editor (.alcar)")
        .open(&mut still_open)
        .resizable(true)
        .default_width(480.0)
        .show(ctx, |ui| {
            if let Some(path) = &app.car_preset_editor.loaded_path {
                ui.small(format!("📄 {}", path));
                ui.separator();
            }
            let e = &mut app.car_preset_editor.edit;

            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                ui.collapsing("Метаданные", |ui| {
                    ui.horizontal(|ui| { ui.label("Марка:"); ui.text_edit_singleline(&mut e.brand); ui.label("Модель:"); ui.text_edit_singleline(&mut e.model); });
                    ui.horizontal(|ui| { ui.label("Меш кузова (.altex путь):"); ui.text_edit_singleline(&mut e.mesh_path); });
                    ui.horizontal(|ui| {
                        ui.label("Год:"); ui.add(egui::DragValue::new(&mut e.year).range(1900..=2100));
                        ui.label("Цена:"); ui.add(egui::DragValue::new(&mut e.price).range(0..=100_000_000));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Расход л/100км:"); ui.add(egui::DragValue::new(&mut e.fuel_consumption).speed(0.1).range(0.0..=100.0));
                        ui.label("Бак л:"); ui.add(egui::DragValue::new(&mut e.fuel_tank).speed(1.0).range(0.0..=500.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Категория (код):"); ui.add(egui::DragValue::new(&mut e.category).range(0..=255));
                        ui.label("Редкость (код):"); ui.add(egui::DragValue::new(&mut e.rarity).range(0..=255));
                    });
                });

                ui.collapsing("Двигатель / трансмиссия", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Мощность л.с.:"); ui.add(egui::DragValue::new(&mut e.engine_power).speed(1.0).range(0.0..=5000.0));
                        ui.label("Крутящий момент:"); ui.add(egui::DragValue::new(&mut e.torque).speed(1.0).range(0.0..=5000.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Макс. об/мин:"); ui.add(egui::DragValue::new(&mut e.max_rpm).speed(10.0).range(1000.0..=20000.0));
                        ui.label("Холостой ход:"); ui.add(egui::DragValue::new(&mut e.idle_rpm).speed(10.0).range(300.0..=3000.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Передач:"); ui.add(egui::DragValue::new(&mut e.gears).range(1..=10));
                        ui.label("Главная передача:"); ui.add(egui::DragValue::new(&mut e.final_drive).speed(0.05).range(1.0..=10.0));
                    });
                });

                ui.collapsing("Шасси / физика", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Масса кг:"); ui.add(egui::DragValue::new(&mut e.weight).speed(10.0).range(50.0..=50000.0));
                        ui.label("Радиус колеса:"); ui.add(egui::DragValue::new(&mut e.wheel_radius).speed(0.01).range(0.05..=2.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Жёсткость подвески:"); ui.add(egui::DragValue::new(&mut e.suspension_stiffness).speed(100.0).range(0.0..=200000.0));
                        ui.label("Демпфирование:"); ui.add(egui::DragValue::new(&mut e.suspension_damping).speed(10.0).range(0.0..=20000.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Тормоза:"); ui.add(egui::DragValue::new(&mut e.brake_power).speed(50.0).range(0.0..=50000.0));
                        ui.label("Ручник:"); ui.add(egui::DragValue::new(&mut e.handbrake_power).speed(50.0).range(0.0..=50000.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Угол поворота руля °:"); ui.add(egui::DragValue::new(&mut e.steering_angle).speed(0.5).range(1.0..=90.0));
                        ui.label("Радиус разворота:"); ui.add(egui::DragValue::new(&mut e.turning_radius).speed(0.1).range(1.0..=30.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Коэф. сопротивления:"); ui.add(egui::DragValue::new(&mut e.drag_coefficient).speed(0.01).range(0.0..=2.0));
                        ui.label("Прижимная сила:"); ui.add(egui::DragValue::new(&mut e.downforce).speed(1.0).range(0.0..=2000.0));
                    });
                });

                ui.collapsing("Динамика (справочно, движком не пересчитывается)", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Макс. скорость км/ч:"); ui.add(egui::DragValue::new(&mut e.top_speed).speed(1.0).range(0.0..=600.0));
                        ui.label("0-100 км/ч, сек:"); ui.add(egui::DragValue::new(&mut e.acceleration_0_100).speed(0.1).range(0.5..=60.0));
                    });
                });

                ui.collapsing("Огни (число фар — позиции пока по умолчанию)", |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Фары:"); ui.add(egui::DragValue::new(&mut e.headlight_count).range(0..=4));
                        ui.label("Стопы:"); ui.add(egui::DragValue::new(&mut e.taillight_count).range(0..=4));
                        ui.label("Поворотники:"); ui.add(egui::DragValue::new(&mut e.blinker_count).range(0..=4));
                    });
                });
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("🆕 Сбросить на дефолт").clicked() {
                    *e = crate::converters::alcar::CarPresetEdit::default();
                }
                if ui.button("📂 Загрузить .alcar...").clicked() {
                    do_load = true;
                }
                if ui.button("💾 Сохранить как .alcar...").clicked() {
                    do_save = true;
                }
            });
        });

    if do_load {
        app.load_car_preset_dialog();
    }
    if do_save {
        app.save_car_preset_dialog();
    }
    if !still_open {
        app.car_preset_editor.open = false;
    }
}
