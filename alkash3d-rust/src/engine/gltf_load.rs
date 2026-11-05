//! ДОБАВЛЕНО (модели glTF): загрузка `crate::gltf::Model` в меши и текстуры движка.
//! Каждый примитив — свой меш; узлы уже запечены в вершины, поэтому модель
//! ставится в сцену обычными `MeshInstance` с одним положением на всю модель.

use super::{AlkashEngine, Mesh, Vertex};
use crate::gltf::{AlphaMode, Image, Model};
use std::collections::HashMap;

/// Модель в движке: меши частей и её габариты (метры, как в файле).
#[derive(Clone, Debug)]
pub struct GltfModel {
    pub meshes: Vec<usize>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

impl AlkashEngine {
    /// Загрузить .gltf или .glb. Текстуры одного файла, нужные нескольким частям,
    /// загружаются один раз.
    pub fn load_gltf(&mut self, path: &str) -> Result<GltfModel, String> {
        let model = crate::gltf::load(std::path::Path::new(path))?;
        let out = self.add_gltf(&model);
        println!("[glTF] {}: {} частей, {} материалов, {} текстур", path, model.primitives.len(), model.materials.len(), model.textures.iter().filter(|t| t.is_some()).count());
        Ok(out)
    }

    /// Загрузить уже прочитанную модель.
    pub fn add_gltf(&mut self, model: &Model) -> GltfModel {
        // ключ: номер текстуры и что из неё сделано (у одной картинки разные роли)
        let mut cache: HashMap<(usize, u32, [u32; 2]), Option<u32>> = HashMap::new();
        let mut meshes = Vec::new();
        for prim in &model.primitives {
            let mat = &model.materials[prim.material];
            let c = mat.base_color;
            let vertices: Vec<Vertex> = prim
                .vertices
                .iter()
                .map(|v| {
                    // цвет вершины * коэффициент — тот же множитель альбедо, что в glTF
                    Vertex::with_normal_uv_tangent(
                        v.position[0], v.position[1], v.position[2],
                        v.normal[0], v.normal[1], v.normal[2],
                        v.color[0] * c[0], v.color[1] * c[1], v.color[2] * c[2], v.color[3] * c[3],
                        v.uv[0], v.uv[1],
                        v.tangent,
                    )
                })
                .collect();
            let mut mesh = match Mesh::from_vertices_and_indices(&vertices, &prim.indices) {
                Ok(m) => m,
                Err(e) => {
                    eprintln!("[glTF] часть «{}» не загружена: {:?}", prim.node, e);
                    continue;
                }
            };
            // glTF: шероховатость и металл — коэффициенты, текстура (если есть) их умножает
            mesh.material_metallic = mat.metallic;
            mesh.material_roughness = mat.roughness;
            mesh.material_emissive = mat.emissive;
            match mat.alpha {
                AlphaMode::Opaque => {}
                AlphaMode::Mask(cutoff) => mesh.alpha_cutoff = cutoff,
                AlphaMode::Blend => mesh.blend = true,
            }
            if let Some(t) = mat.base_color_texture {
                mesh.albedo_srv_index = *cache.entry((t, 0, [0, 0])).or_insert_with(|| model.textures.get(t).and_then(|i| i.as_ref()).and_then(|img| self.create_texture_rgba(img.width, img.height, &img.rgba)));
            }
            if let Some(t) = mat.normal_texture {
                let key = (t, 1, [mat.normal_scale.to_bits(), 0]);
                mesh.normal_srv_index = *cache.entry(key).or_insert_with(|| {
                    let img = model.textures.get(t).and_then(|i| i.as_ref())?;
                    let pixels = normal_pixels(img, mat.normal_scale);
                    self.create_texture_rgba(img.width, img.height, &pixels)
                });
            }
            if let Some(t) = mat.metallic_roughness_texture {
                let key = (t, 2, [mat.metallic.to_bits(), mat.roughness.to_bits()]);
                mesh.mr_srv_index = *cache.entry(key).or_insert_with(|| {
                    let img = model.textures.get(t).and_then(|i| i.as_ref())?;
                    let pixels = metallic_roughness_pixels(img, mat.metallic, mat.roughness);
                    self.create_texture_rgba(img.width, img.height, &pixels)
                });
            }
            meshes.push(self.add_mesh(mesh));
        }
        GltfModel { meshes, bounds_min: model.bounds_min, bounds_max: model.bounds_max }
    }
}

/// Карта нормалей glTF с учётом `normalTexture.scale`: XY умножаются, вектор
/// нормируется заново. Альфа — 255: высоты для рельефа в glTF нет.
fn normal_pixels(img: &Image, scale: f32) -> Vec<u8> {
    let mut out = img.rgba.clone();
    for p in out.chunks_exact_mut(4) {
        if scale != 1.0 {
            let mut n = [p[0] as f32 / 127.5 - 1.0, p[1] as f32 / 127.5 - 1.0, p[2] as f32 / 127.5 - 1.0];
            n[0] *= scale;
            n[1] *= scale;
            let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt().max(1e-6);
            for k in 0..3 {
                p[k] = ((n[k] / l * 0.5 + 0.5) * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
        p[3] = 255;
    }
    out
}

/// glTF: G — шероховатость, B — металл; движок читает R — металл, G — шероховатость.
/// Коэффициенты материала запекаются в пиксели.
fn metallic_roughness_pixels(img: &Image, metallic: f32, roughness: f32) -> Vec<u8> {
    let mut out = img.rgba.clone();
    for p in out.chunks_exact_mut(4) {
        let (r, m) = (p[1] as f32 * roughness, p[2] as f32 * metallic);
        p[0] = m.round().clamp(0.0, 255.0) as u8;
        p[1] = r.round().clamp(0.0, 255.0) as u8;
        p[2] = 0;
        p[3] = 255;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metallic_roughness_channels_are_swapped_into_engine_order() {
        let img = Image { width: 1, height: 1, rgba: vec![9, 200, 100, 7] };
        let p = metallic_roughness_pixels(&img, 0.5, 1.0);
        assert_eq!(&p[..], &[50, 200, 0, 255]);
    }

    #[test]
    fn normal_scale_zero_flattens_the_map() {
        let img = Image { width: 1, height: 1, rgba: vec![200, 60, 220, 255] };
        let p = normal_pixels(&img, 0.0);
        assert_eq!((p[0], p[1]), (128, 128));
        assert!(p[2] > 250);
    }
}
