//! Управление Pipeline State Objects

use anyhow::{Result, anyhow};
use std::ffi::c_void;
use std::collections::HashMap;
use crate::ffi::AlkashDll;

pub struct PipelineManager {
    dll: &'static AlkashDll,
    device: *mut c_void,
    root_signature: *mut c_void,
    pipelines: HashMap<String, *mut c_void>,
}

impl PipelineManager {
    pub fn new(dll: &'static AlkashDll, device: *mut c_void, root_signature: *mut c_void) -> Self {
        Self {
            dll,
            device,
            root_signature,
            pipelines: HashMap::new(),
        }
    }

    pub fn create_default_pso(&mut self) -> Result<*mut c_void> {
        unsafe {
            let pso = (self.dll.create_advanced_pso)(self.device, self.root_signature);
            if pso.is_null() {
                return Err(anyhow!("Failed to create default PSO"));
            }
            self.pipelines.insert("default".to_string(), pso);
            Ok(pso)
        }
    }

    pub fn create_wireframe_pso(&mut self) -> Result<*mut c_void> {
        unsafe {
            let pso = (self.dll.create_pso)(self.device, self.root_signature, 3);
            if pso.is_null() {
                return Err(anyhow!("Failed to create wireframe PSO"));
            }
            self.pipelines.insert("wireframe".to_string(), pso);
            Ok(pso)
        }
    }

    pub fn get(&self, name: &str) -> Option<*mut c_void> {
        self.pipelines.get(name).copied()
    }

    pub fn cleanup(&mut self) {
        unsafe {
            for pso in self.pipelines.values() {
                if !pso.is_null() {
                    (self.dll.release_resource)(*pso);
                }
            }
        }
        self.pipelines.clear();
    }
}

impl Drop for PipelineManager {
    fn drop(&mut self) {
        self.cleanup();
    }
}