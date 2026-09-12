// src/editor/gizmo3d.rs
//
// ДОБАВЛЕНО (реальный интерактивный gizmo во вьюпорте — до этого
// `editor::Gizmo`/`GizmoMode`/`GizmoAxis` в gizmo.rs существовали как
// структуры данных, но НИ ОДНА функция в приложении их не вызывала —
// `current_tool` переключался кнопками тулбара, но ни на что не влиял:
// подвинуть/повернуть/отмасштабировать объект в 3D можно было только вводом
// чисел в инспекторе. Этот модуль — только СОСТОЯНИЕ перетаскивания, см.
// EditorApp::handle_gizmo_input/draw_gizmo в app.rs, где живёт вся
// экранная проекция и математика, — намеренно НЕ трогаем старый gizmo.rs
// (его типы отражают другую, так и не подключённую модель взаимодействия).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GizmoAxisSel {
    X,
    Y,
    Z,
}

impl GizmoAxisSel {
    pub fn world_dir(&self) -> crate::math::Vec3 {
        match self {
            GizmoAxisSel::X => crate::math::Vec3::RIGHT,
            GizmoAxisSel::Y => crate::math::Vec3::UP,
            GizmoAxisSel::Z => crate::math::Vec3::FORWARD,
        }
    }

    pub fn color(&self) -> egui::Color32 {
        match self {
            GizmoAxisSel::X => egui::Color32::from_rgb(224, 64, 64),
            GizmoAxisSel::Y => egui::Color32::from_rgb(64, 200, 96),
            GizmoAxisSel::Z => egui::Color32::from_rgb(80, 128, 232),
        }
    }
}

/// Активное перетаскивание одной оси гизмо — живёт от `pointer down` на
/// хэндле до `pointer up`. `tool`/`axis` фиксируются в момент начала
/// перетаскивания (даже если пользователь переключит инструмент/выделение
/// на середине жеста — драг доводится до конца по исходным правилам, что
/// куда предсказуемее внезапной смены поведения на лету).
#[derive(Debug, Clone)]
pub struct GizmoDrag {
    pub axis: GizmoAxisSel,
    pub tool: crate::editor::EditorTool,
    pub last_mouse: egui::Pos2,
}
