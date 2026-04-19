//! Инструменты редактора

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTool {
    Select,
    Move,
    Rotate,
    Scale,
    Pan,
    Zoom,
}

impl Default for EditorTool {
    fn default() -> Self {
        Self::Select
    }
}

impl EditorTool {
    pub fn name(&self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Move => "Move",
            Self::Rotate => "Rotate",
            Self::Scale => "Scale",
            Self::Pan => "Pan",
            Self::Zoom => "Zoom",
        }
    }

    pub fn shortcut(&self) -> &'static str {
        match self {
            Self::Select => "Q",
            Self::Move => "W",
            Self::Rotate => "E",
            Self::Scale => "R",
            Self::Pan => "G",
            Self::Zoom => "Z",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Select => "🖱",
            Self::Move => "↔",
            Self::Rotate => "🔄",
            Self::Scale => "⤢",
            Self::Pan => "✋",
            Self::Zoom => "🔍",
        }
    }
}