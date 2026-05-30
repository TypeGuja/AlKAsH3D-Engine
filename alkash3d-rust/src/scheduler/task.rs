//! Определение задач и приоритетов

/// Приоритет задачи
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskPriority {
    /// Критические — не могут быть отложены (рендер)
    Critical = 3,
    /// Высокий приоритет — физика, каллинг
    High = 2,
    /// Обычный — обновление скриптов, анимаций
    Normal = 1,
    /// Низкий — фоновые загрузки, подготовка ассетов
    Low = 0,
}

impl TaskPriority {
    /// Сколько ядер нужно для задачи
    pub fn cores(&self) -> usize {
        match self {
            Self::Critical => 1,
            Self::High => 1,
            Self::Normal => 1,
            Self::Low => 1,
        }
    }
}

/// Описание задачи
#[derive(Debug, Clone, Copy)]
pub struct Task {
    pub id: u32,
    pub priority: TaskPriority,
    pub estimated_ms: f32,
}

impl Task {
    pub fn new(id: u32, priority: TaskPriority) -> Self {
        Self {
            id,
            priority,
            estimated_ms: 0.0,
        }
    }

    pub fn with_cost(mut self, ms: f32) -> Self {
        self.estimated_ms = ms;
        self
    }
}

/// Тайминги одного кадра
#[derive(Debug, Default, Clone, Copy)]
pub struct FrameTimings {
    pub broad_phase_ms: f32,
    pub narrow_phase_ms: f32,
    pub solver_ms: f32,
    pub render_ms: f32,
    pub culling_ms: f32,
    pub physics_ms: f32,
    pub scripts_ms: f32,
    pub audio_ms: f32,
    pub streaming_ms: f32,
}