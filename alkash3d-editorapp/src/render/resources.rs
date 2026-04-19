//! Управление ресурсами рендеринга

use anyhow::{Result, anyhow};
use std::ffi::c_void;
use std::collections::HashMap;
use crate::ffi::AlkashDll;

pub struct ResourceManager {
    dll: &'static AlkashDll,
    device: *mut c_void,
    buffers: HashMap<String, BufferHandle>,
    textures: HashMap<String, TextureHandle>,
}

#[derive(Clone)]
pub struct BufferHandle {
    pub ptr: *mut c_void,
    pub size: usize,
    pub gpu_address: u64,
}

#[derive(Clone)]
pub struct TextureHandle {
    pub ptr: *mut c_void,
    pub width: u32,
    pub height: u32,
    pub format: u32,
    pub mip_levels: u32,
    pub srv_handle: u64,
}

impl ResourceManager {
    pub fn new(dll: &'static AlkashDll, device: *mut c_void) -> Self {
        Self {
            dll,
            device,
            buffers: HashMap::new(),
            textures: HashMap::new(),
        }
    }

    pub fn create_vertex_buffer(
        &mut self,
        name: &str,
        data: &[u8],
    ) -> Result<BufferHandle> {
        unsafe {
            let ptr = (self.dll.create_buffer)(self.device, data.len(), 1);
            if ptr.is_null() {
                return Err(anyhow!("Failed to create vertex buffer"));
            }

            let upload_ptr = (self.dll.create_buffer)(self.device, data.len(), 0);
            if upload_ptr.is_null() {
                (self.dll.release_resource)(ptr);
                return Err(anyhow!("Failed to create upload buffer"));
            }

            (self.dll.update_subresource)(upload_ptr, data.as_ptr() as *const c_void, data.len());
            (self.dll.release_resource)(upload_ptr);

            let gpu_address = (self.dll.get_buffer_gpu_address)(ptr);

            let handle = BufferHandle {
                ptr,
                size: data.len(),
                gpu_address,
            };

            self.buffers.insert(name.to_string(), handle.clone());
            Ok(handle)
        }
    }

    pub fn create_index_buffer(
        &mut self,
        name: &str,
        data: &[u32],
    ) -> Result<BufferHandle> {
        unsafe {
            let bytes = std::slice::from_raw_parts(
                data.as_ptr() as *const u8,
                data.len() * 4,
            );
            self.create_vertex_buffer(name, bytes)
        }
    }

    pub fn get_buffer(&self, name: &str) -> Option<&BufferHandle> {
        self.buffers.get(name)
    }

    pub fn cleanup(&mut self) {
        unsafe {
            for handle in self.buffers.values() {
                if !handle.ptr.is_null() {
                    (self.dll.release_resource)(handle.ptr);
                }
            }
            for handle in self.textures.values() {
                if !handle.ptr.is_null() {
                    (self.dll.release_resource)(handle.ptr);
                }
            }
        }
        self.buffers.clear();
        self.textures.clear();
    }
}

impl Drop for ResourceManager {
    fn drop(&mut self) {
        self.cleanup();
    }
}