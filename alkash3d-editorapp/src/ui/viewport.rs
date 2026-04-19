//! 3D вьюпорт

use egui::*;

pub struct ViewportWidget {
    pub rect: Rect,
    pub hovered: bool,
    pub focused: bool,
}

impl ViewportWidget {
    pub fn new() -> Self {
        Self {
            rect: Rect::NOTHING,
            hovered: false,
            focused: false,
        }
    }

    pub fn show<R>(
        &mut self,
        ui: &mut Ui,
        add_contents: impl FnOnce(&mut Ui, Rect) -> R,
    ) -> egui::InnerResponse<R> {
        let (rect, response) = ui.allocate_exact_size(
            ui.available_size(),
            egui::Sense::click_and_drag(),
        );

        self.rect = rect;
        self.hovered = response.hovered();
        self.focused = response.has_focus();

        let inner_response = add_contents(ui, rect);

        egui::InnerResponse::new(inner_response, response)
    }
}

impl Default for ViewportWidget {
    fn default() -> Self {
        Self::new()
    }
}