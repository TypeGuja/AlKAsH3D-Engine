//! Система выделения объектов

use crate::math::{Ray, Vec3, AABB, Vec2, Camera};
use crate::scene::{Scene, GameObject};
use uuid::Uuid;
use egui::*;

pub struct SelectionSystem {
    pub selection_mode: SelectionMode,
    pub marquee_active: bool,
    pub marquee_start: Vec2,
    pub marquee_end: Vec2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Single,
    Add,
    Subtract,
    Toggle,
}

impl Default for SelectionSystem {
    fn default() -> Self {
        Self {
            selection_mode: SelectionMode::Single,
            marquee_active: false,
            marquee_start: Vec2::ZERO,
            marquee_end: Vec2::ZERO,
        }
    }
}

impl SelectionSystem {
    pub fn raycast_select(
        &self,
        ray: Ray,
        scene: &mut Scene,
        mode: SelectionMode,
    ) -> Option<Uuid> {
        let mut closest_dist = f32::MAX;
        let mut closest_id = None;

        for obj in scene.objects.values() {
            if let Some(bounds) = self.get_object_bounds(obj) {
                if let Some(t) = ray.intersect_aabb(bounds.min, bounds.max) {
                    if t < closest_dist {
                        closest_dist = t;
                        closest_id = Some(obj.id);
                    }
                }
            }
        }

        if let Some(id) = closest_id {
            match mode {
                SelectionMode::Single => {
                    scene.selection.clear();
                    scene.selection.insert(id);
                }
                SelectionMode::Add => {
                    scene.selection.insert(id);
                }
                SelectionMode::Subtract => {
                    scene.selection.remove(&id);
                }
                SelectionMode::Toggle => {
                    if scene.selection.contains(&id) {
                        scene.selection.remove(&id);
                    } else {
                        scene.selection.insert(id);
                    }
                }
            }
        } else if mode == SelectionMode::Single {
            scene.selection.clear();
        }

        closest_id
    }

    pub fn marquee_select(
        &mut self,
        rect: Rect,
        camera: &Camera,
        scene: &mut Scene,
    ) {
        let mut selected = Vec::new();

        for obj in scene.objects.values() {
            let screen_pos = world_to_screen(
                obj.world_position(),
                camera,
                rect.width(),
                rect.height(),
            );

            if rect.contains(screen_pos) {
                selected.push(obj.id);
            }
        }

        if !selected.is_empty() {
            scene.selection.clear();
            scene.selection.extend(selected);
        }
    }

    fn get_object_bounds(&self, obj: &GameObject) -> Option<AABB> {
        let size = obj.transform.scale;
        let half = size * 0.5;
        Some(AABB::new(
            obj.transform.translation - half,
            obj.transform.translation + half,
        ))
    }
}

fn world_to_screen(pos: Vec3, camera: &Camera, screen_width: f32, screen_height: f32) -> egui::Pos2 {
    let clip = camera.view_projection_matrix() * pos.extend(1.0);

    if clip.w <= 0.0 {
        return egui::Pos2::new(screen_width / 2.0, screen_height / 2.0);
    }

    let ndc_x = clip.x / clip.w;
    let ndc_y = clip.y / clip.w;

    egui::Pos2::new(
        (ndc_x * 0.5 + 0.5) * screen_width,
        (0.5 - ndc_y * 0.5) * screen_height,
    )
}