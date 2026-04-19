//! Пользовательские виджеты

use egui::*;

pub struct TransformWidget {
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
}

impl TransformWidget {
    pub fn new() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }

    pub fn from_transform(transform: &crate::math::Transform) -> Self {
        let (roll, pitch, yaw) = transform.rotation.to_euler(glam::EulerRot::XYZ);
        Self {
            position: transform.translation.to_array(),
            rotation: [roll.to_degrees(), pitch.to_degrees(), yaw.to_degrees()],
            scale: transform.scale.to_array(),
        }
    }

    pub fn show(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;

        ui.collapsing("Transform", |ui| {
            ui.horizontal(|ui| {
                ui.label("Position:");
                changed |= ui.add(egui::DragValue::new(&mut self.position[0]).speed(0.1).prefix("X ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut self.position[1]).speed(0.1).prefix("Y ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut self.position[2]).speed(0.1).prefix("Z ")).changed();
            });

            ui.horizontal(|ui| {
                ui.label("Rotation:");
                changed |= ui.add(egui::DragValue::new(&mut self.rotation[0]).speed(1.0).prefix("X ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut self.rotation[1]).speed(1.0).prefix("Y ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut self.rotation[2]).speed(1.0).prefix("Z ")).changed();
            });

            ui.horizontal(|ui| {
                ui.label("Scale:");
                changed |= ui.add(egui::DragValue::new(&mut self.scale[0]).speed(0.1).prefix("X ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut self.scale[1]).speed(0.1).prefix("Y ")).changed();
                changed |= ui.add(egui::DragValue::new(&mut self.scale[2]).speed(0.1).prefix("Z ")).changed();
            });

            if ui.button("Reset").clicked() {
                self.position = [0.0; 3];
                self.rotation = [0.0; 3];
                self.scale = [1.0; 3];
                changed = true;
            }
        });

        changed
    }

    pub fn to_transform(&self) -> crate::math::Transform {
        crate::math::Transform {
            translation: glam::Vec3::from_array(self.position),
            rotation: glam::Quat::from_euler(
                glam::EulerRot::XYZ,
                self.rotation[0].to_radians(),
                self.rotation[1].to_radians(),
                self.rotation[2].to_radians(),
            ),
            scale: glam::Vec3::from_array(self.scale),
        }
    }
}

impl Default for TransformWidget {
    fn default() -> Self {
        Self::new()
    }
}

pub struct ObjectListWidget {
    pub objects: Vec<(String, bool, bool)>, // (name, visible, selected)
}

impl ObjectListWidget {
    pub fn new() -> Self {
        Self {
            objects: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.objects.clear();
    }

    pub fn add(&mut self, name: String, visible: bool) {
        self.objects.push((name, visible, false));
    }

    pub fn show(&mut self, ui: &mut Ui) -> Vec<usize> {
        let mut selected_indices = Vec::new();
        let mut toggles = Vec::new();
        let mut selections = Vec::new();

        for (i, (name, visible, selected)) in self.objects.iter().enumerate() {
            let mut new_visible = *visible;
            let new_selected = *selected;

            ui.horizontal(|ui| {
                let eye_text = if new_visible { "👁" } else { "👁‍🗨" };
                if ui.selectable_label(false, eye_text).clicked() {
                    new_visible = !new_visible;
                    toggles.push((i, new_visible));
                }

                let response = ui.selectable_label(new_selected, name.as_str());

                if response.clicked() {
                    let shift = ui.input(|i| i.modifiers.shift);
                    selections.push((i, shift));
                }
            });

            if new_selected {
                selected_indices.push(i);
            }
        }

        for (i, visible) in toggles {
            self.objects[i].1 = visible;
        }

        for (i, shift) in selections {
            if !shift {
                for obj in &mut self.objects {
                    obj.2 = false;
                }
            }
            self.objects[i].2 = true;
        }

        selected_indices.clear();
        for (i, (_, _, selected)) in self.objects.iter().enumerate() {
            if *selected {
                selected_indices.push(i);
            }
        }

        selected_indices
    }

    pub fn get_selected(&self) -> Vec<usize> {
        self.objects.iter()
            .enumerate()
            .filter(|(_, (_, _, sel))| *sel)
            .map(|(i, _)| i)
            .collect()
    }

    pub fn get_visible_names(&self) -> Vec<String> {
        self.objects.iter()
            .filter(|(_, visible, _)| *visible)
            .map(|(name, _, _)| name.clone())
            .collect()
    }
}

impl Default for ObjectListWidget {
    fn default() -> Self {
        Self::new()
    }
}