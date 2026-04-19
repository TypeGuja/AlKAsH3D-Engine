//! Система Undo/Redo

use std::collections::VecDeque;
use crate::scene::{Scene, GameObject};
use crate::math::Vec3;
use uuid::Uuid;

pub trait Command: Send + Sync {
    fn execute(&mut self, scene: &mut Scene);
    fn undo(&mut self, scene: &mut Scene);
    fn description(&self) -> String;
}

pub struct CommandHistory {
    undo_stack: VecDeque<Box<dyn Command>>,
    redo_stack: VecDeque<Box<dyn Command>>,
    max_size: usize,
}

impl CommandHistory {
    pub fn new(max_size: usize) -> Self {
        Self {
            undo_stack: VecDeque::with_capacity(max_size),
            redo_stack: VecDeque::new(),
            max_size,
        }
    }

    pub fn execute(&mut self, mut command: Box<dyn Command>, scene: &mut Scene) {
        command.execute(scene);

        if self.undo_stack.len() >= self.max_size {
            self.undo_stack.pop_back();
        }
        self.undo_stack.push_front(command);
        self.redo_stack.clear();
    }

    pub fn undo(&mut self, scene: &mut Scene) -> bool {
        if let Some(mut command) = self.undo_stack.pop_front() {
            command.undo(scene);
            self.redo_stack.push_front(command);
            true
        } else {
            false
        }
    }

    pub fn redo(&mut self, scene: &mut Scene) -> bool {
        if let Some(mut command) = self.redo_stack.pop_front() {
            command.execute(scene);
            self.undo_stack.push_front(command);
            true
        } else {
            false
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn clear(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
    }
}

// Команды

pub struct MoveObjectsCommand {
    object_ids: Vec<Uuid>,
    old_positions: Vec<Vec3>,
    new_positions: Vec<Vec3>,
}

impl MoveObjectsCommand {
    pub fn new(object_ids: Vec<Uuid>, old_positions: Vec<Vec3>, new_positions: Vec<Vec3>) -> Self {
        Self {
            object_ids,
            old_positions,
            new_positions,
        }
    }
}

impl Command for MoveObjectsCommand {
    fn execute(&mut self, scene: &mut Scene) {
        for (id, pos) in self.object_ids.iter().zip(&self.new_positions) {
            if let Some(obj) = scene.get_object_mut(*id) {
                obj.transform.translation = *pos;
            }
        }
    }

    fn undo(&mut self, scene: &mut Scene) {
        for (id, pos) in self.object_ids.iter().zip(&self.old_positions) {
            if let Some(obj) = scene.get_object_mut(*id) {
                obj.transform.translation = *pos;
            }
        }
    }

    fn description(&self) -> String {
        format!("Move {} object(s)", self.object_ids.len())
    }
}

pub struct DeleteObjectsCommand {
    objects: Vec<GameObject>,
}

impl DeleteObjectsCommand {
    pub fn new(objects: Vec<GameObject>) -> Self {
        Self { objects }
    }
}

impl Command for DeleteObjectsCommand {
    fn execute(&mut self, scene: &mut Scene) {
        for obj in &self.objects {
            scene.remove_object(obj.id);
        }
    }

    fn undo(&mut self, scene: &mut Scene) {
        for obj in &self.objects {
            scene.add_object(obj.clone());
        }
    }

    fn description(&self) -> String {
        format!("Delete {} object(s)", self.objects.len())
    }
}

pub struct CreateObjectCommand {
    object: GameObject,
}

impl CreateObjectCommand {
    pub fn new(object: GameObject) -> Self {
        Self { object }
    }
}

impl Command for CreateObjectCommand {
    fn execute(&mut self, scene: &mut Scene) {
        scene.add_object(self.object.clone());
    }

    fn undo(&mut self, scene: &mut Scene) {
        scene.remove_object(self.object.id);
    }

    fn description(&self) -> String {
        format!("Create '{}'", self.object.name)
    }
}