// src/assets/city_import.rs
//
// ДОБАВЛЕНО (по прямому запросу пользователя: "сделай так чтобы можно было
// выбрать директорию с этим городом и эдитор сам подтянул текстуры"):
// импорт папки с картой, нарезанной на OBJ-чанки (формат tools/samara_map:
// `manifest.json` + `chunks/chunk_<gx>_<gz>.obj` + `*.mtl` + `textures/`).
//
// Обычный `AssetLibrary::parse_obj` для этого не годится: он склеивает все
// группы файла в ОДИН меш и игнорирует .mtl. Здесь каждая пара
// (квартал 2x2 км, материал) становится отдельным меш-объектом со своим
// материалом эдитора, а материал получает все три PBR-карты из .mtl:
// albedo (`map_Kd`), normal (`map_Bump`/`norm`) и metallic-roughness
// (собирается из `map_Pr` + скалярного `Pm`). Экспорт в .alworld потом
// режет эти объекты по чанкам 256 м сам (`split_mesh_by_chunk`), а
// текстуры пишет один раз на весь мир (`converters::altex::SharedTextures`).
//
// Вершины в чанках — уже в мировых координатах, объекты ставятся с
// единичной трансформацией.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{anyhow, Context, Result};
use rayon::prelude::*;

use crate::material::{Material, TextureAsset};
use crate::math::Vec3;
use crate::mesh::Mesh;

/// Сколько чанков 256 м сводить в один объект сцены по каждой оси: 8x8 =
/// квартал 2x2 км. Мельче — десятки тысяч объектов (иерархия и draw calls
/// вьюпорта захлёбываются), крупнее — неудобно выделять/удалять районы.
pub const CHUNKS_PER_GROUP: i32 = 8;

#[derive(Clone, Debug)]
pub struct CityChunk {
    pub path: PathBuf,
    pub gx: i32,
    pub gz: i32,
    pub center: [f32; 2],
    pub triangles: u64,
}

/// Что лежит в выбранной папке — читается сразу при выборе, до импорта,
/// чтобы диалог мог показать размер и оценку памяти.
#[derive(Clone, Debug)]
pub struct CityInfo {
    pub dir: PathBuf,
    pub name: String,
    pub mtl_path: PathBuf,
    pub chunks: Vec<CityChunk>,
    pub chunk_size: f32,
}

impl CityInfo {
    pub fn open(dir: &Path) -> Result<Self> {
        let chunks_dir = dir.join("chunks");
        if !chunks_dir.is_dir() {
            return Err(anyhow!("в папке '{}' нет подпапки chunks/ с OBJ-чанками", dir.display()));
        }
        let mtl_path = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .find(|p| p.extension().map_or(false, |x| x.eq_ignore_ascii_case("mtl")))
            .ok_or_else(|| anyhow!("в папке '{}' нет .mtl-файла с материалами", dir.display()))?;

        let mut name = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "City".into());
        let mut chunk_size = 256.0f32;
        let mut tri_by_file: HashMap<String, u64> = HashMap::new();
        if let Ok(text) = std::fs::read_to_string(dir.join("manifest.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                if let Some(n) = v.get("name").and_then(|n| n.as_str()) {
                    name = n.to_string();
                }
                if let Some(cs) = v.get("chunk_size").and_then(|c| c.as_f64()) {
                    chunk_size = cs as f32;
                }
                for c in v.get("chunks").and_then(|c| c.as_array()).into_iter().flatten() {
                    if let (Some(f), Some(t)) = (c.get("file").and_then(|f| f.as_str()), c.get("triangles").and_then(|t| t.as_u64())) {
                        let fname = Path::new(f).file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                        tri_by_file.insert(fname, t);
                    }
                }
            }
        }

        let mut chunks = Vec::new();
        for e in std::fs::read_dir(&chunks_dir)? {
            let path = e?.path();
            let Some(stem) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
            if !path.extension().map_or(false, |x| x.eq_ignore_ascii_case("obj")) {
                continue;
            }
            // chunk_<gx>_<gz>
            let parts: Vec<&str> = stem.split('_').collect();
            if parts.len() != 3 || parts[0] != "chunk" {
                continue;
            }
            let (Ok(gx), Ok(gz)) = (parts[1].parse::<i32>(), parts[2].parse::<i32>()) else { continue };
            let fname = path.file_name().unwrap().to_string_lossy().into_owned();
            // без манифеста — грубая оценка по размеру файла (~110 байт на треугольник в этом OBJ)
            let triangles = tri_by_file
                .get(&fname)
                .copied()
                .unwrap_or_else(|| std::fs::metadata(&path).map(|m| m.len() / 110).unwrap_or(0));
            chunks.push(CityChunk {
                path,
                gx,
                gz,
                center: [(gx as f32 + 0.5) * chunk_size, (gz as f32 + 0.5) * chunk_size],
                triangles,
            });
        }
        if chunks.is_empty() {
            return Err(anyhow!("в '{}' нет файлов chunk_<x>_<z>.obj", chunks_dir.display()));
        }
        chunks.sort_by_key(|c| (c.gx, c.gz));
        Ok(Self { dir: dir.to_path_buf(), name, mtl_path, chunks, chunk_size })
    }

    /// Чанки в радиусе `radius_m` от точки (x, z) — `None` = все.
    pub fn select(&self, center: [f32; 2], radius_m: Option<f32>) -> Vec<CityChunk> {
        self.chunks
            .iter()
            .filter(|c| match radius_m {
                None => true,
                Some(r) => {
                    let dx = c.center[0] - center[0];
                    let dz = c.center[1] - center[1];
                    (dx * dx + dz * dz).sqrt() <= r + self.chunk_size * 0.5
                }
            })
            .cloned()
            .collect()
    }

    /// Грубая оценка памяти для диалога: вершин у этих OBJ ~1.15 на
    /// треугольник; в сцене эдитора вершина стоит ~32 Б + индексы, в
    /// очереди загрузки на GPU — ещё столько же временно, на GPU — 36 Б.
    pub fn estimate(chunks: &[CityChunk]) -> (u64, f64, f64) {
        let tris: u64 = chunks.iter().map(|c| c.triangles).sum();
        let verts = tris as f64 * 1.15;
        let ram_gb = (verts * 32.0 + tris as f64 * 12.0) * 2.0 / 1e9;
        let vram_gb = (verts * 36.0 + tris as f64 * 12.0) / 1e9;
        (tris, ram_gb, vram_gb)
    }
}

// ------------------------------------------------------------------ материалы

#[derive(Default, Debug)]
struct MtlEntry {
    name: String,
    kd: Option<[f32; 3]>,
    /// Собственное свечение (`Ke`, линейная яркость в единицах движка) —
    /// светящиеся буквы вывесок у tools/samara_map.
    ke: Option<[f32; 3]>,
    pr: Option<f32>,
    pm: Option<f32>,
    map_kd: Option<PathBuf>,
    map_normal: Option<PathBuf>,
    map_pr: Option<PathBuf>,
}

fn parse_mtl(path: &Path) -> Result<Vec<MtlEntry>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("не удалось прочитать '{}'", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut out: Vec<MtlEntry> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        let mut it = line.split_whitespace();
        let Some(key) = it.next() else { continue };
        // путь к карте — последний токен строки (перед ним бывают опции вроде `-bm 1.0`)
        let last = || line.split_whitespace().last().map(|p| base.join(p));
        let num = |s: Option<&str>| s.and_then(|v| v.parse::<f32>().ok());
        if key == "newmtl" {
            out.push(MtlEntry { name: it.collect::<Vec<_>>().join(" "), ..Default::default() });
            continue;
        }
        let Some(cur) = out.last_mut() else { continue };
        match key {
            "Kd" => {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 3 {
                    cur.kd = Some([v[0], v[1], v[2]]);
                }
            }
            "Ke" => {
                let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
                if v.len() >= 3 {
                    cur.ke = Some([v[0], v[1], v[2]]);
                }
            }
            "Pr" => cur.pr = num(it.next()),
            "Pm" => cur.pm = num(it.next()),
            "map_Kd" => cur.map_kd = last(),
            "norm" | "map_Bump" | "bump" | "map_bump" => {
                if cur.map_normal.is_none() || key == "norm" {
                    cur.map_normal = last();
                }
            }
            "map_Pr" => cur.map_pr = last(),
            _ => {}
        }
    }
    Ok(out)
}

fn load_rgba(path: &Path) -> Result<image::RgbaImage> {
    Ok(image::open(path)
        .with_context(|| format!("не удалось декодировать '{}'", path.display()))?
        .to_rgba8())
}

/// Средний цвет albedo (sRGB, 0..1) — им красится материал во вьюпорте
/// эдитора, который сам текстуры не рисует. Движок `Material::albedo` при
/// наличии albedo-карты не использует (цвет = вершинный цвет * текстура),
/// так что на картинку в игре это не влияет.
fn average_color(img: &image::RgbaImage) -> [f32; 3] {
    let mut sum = [0u64; 3];
    for p in img.pixels() {
        for k in 0..3 {
            sum[k] += p.0[k] as u64;
        }
    }
    let n = (img.width() as u64 * img.height() as u64).max(1) as f32 * 255.0;
    [sum[0] as f32 / n, sum[1] as f32 / n, sum[2] as f32 / n]
}

/// Материалы из .mtl с текстурами. Нормали переводятся из OpenGL-конвенции
/// (+Y вверх по картинке — так их пишут OBJ/Blender и tools/samara_map) в
/// ту, что ждёт движок: эдитор при импорте OBJ переворачивает V (1 - v),
/// тангенты экспорта считаются по перевёрнутой V, поэтому зелёный канал
/// инвертируется. Metallic-roughness пакуется как у движка: R = metallic
/// (скаляр `Pm`), G = roughness (карта `map_Pr` или скаляр `Pr`).
pub fn load_materials(mtl_path: &Path, log: &(dyn Fn(String) + Sync)) -> Result<HashMap<String, Material>> {
    let entries = parse_mtl(mtl_path)?;
    let mats: Vec<(String, Material)> = entries
        .par_iter()
        .map(|e| {
            let mut m = Material { name: e.name.clone(), ..Default::default() };
            m.roughness = e.pr.unwrap_or(0.8);
            m.metallic = e.pm.unwrap_or(0.0);
            let kd = e.kd.unwrap_or([1.0, 1.0, 1.0]);
            m.color = [kd[0], kd[1], kd[2], 1.0];
            if let Some(ke) = e.ke {
                m.emissive = ke;
            }
            if let Some(p) = &e.map_kd {
                match load_rgba(p) {
                    Ok(img) => {
                        let avg = average_color(&img);
                        m.color = [kd[0] * avg[0], kd[1] * avg[1], kd[2] * avg[2], 1.0];
                        let (w, h) = img.dimensions();
                        m.albedo_texture = Some(TextureAsset::from_rgba(w, h, img.into_raw(), Some(p.to_string_lossy().into_owned())));
                    }
                    Err(err) => log(format!("⚠️ {}: {}", e.name, err)),
                }
            }
            if let Some(p) = &e.map_normal {
                match load_rgba(p) {
                    Ok(mut img) => {
                        for px in img.pixels_mut() {
                            px.0[1] = 255 - px.0[1];
                        }
                        let (w, h) = img.dimensions();
                        m.normal_texture = Some(TextureAsset::from_rgba(w, h, img.into_raw(), Some(p.to_string_lossy().into_owned())));
                    }
                    Err(err) => log(format!("⚠️ {}: {}", e.name, err)),
                }
            }
            let metal = (m.metallic.clamp(0.0, 1.0) * 255.0).round() as u8;
            let rough_img = e.map_pr.as_ref().and_then(|p| match load_rgba(p) {
                Ok(img) => Some(img),
                Err(err) => {
                    log(format!("⚠️ {}: {}", e.name, err));
                    None
                }
            });
            if let Some(img) = rough_img {
                let (w, h) = img.dimensions();
                let mut px = Vec::with_capacity((w * h * 4) as usize);
                for p in img.pixels() {
                    px.extend_from_slice(&[metal, p.0[0], 0, 255]);
                }
                m.metallic_roughness_texture = Some(TextureAsset::from_rgba(w, h, px, None));
            }
            (e.name.clone(), m)
        })
        .collect();
    Ok(mats.into_iter().collect())
}

// ------------------------------------------------------------------ геометрия

#[derive(Default)]
struct Accum {
    vertices: Vec<Vec3>,
    normals: Vec<Vec3>,
    uv: Vec<[f32; 2]>,
    /// пусто, пока у материала не встретился цветной OBJ (тогда дополняется белым)
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Accum {
    fn into_mesh(self) -> Mesh {
        let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
        let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
        for v in &self.vertices {
            min = min.min(*v);
            max = max.max(*v);
        }
        // собираем напрямую, без `Mesh::new`: у чанков уже есть честные
        // нормали и UV, пересчитывать их (и копировать массивы) незачем
        Mesh { vertices: self.vertices, indices: self.indices, normals: self.normals, uv: self.uv, colors: self.colors, bounds: (min, max) }
    }
}

fn is_ground_material(name: &str) -> bool {
    !["facade_", "wall_", "roof_", "dome_", "concrete_fence", "fence_", "hedge", "rail_", "road_marking", "lamp_"]
        .iter()
        .any(|p| name.starts_with(p))
}

pub struct CityGroup {
    pub gx: i32,
    pub gz: i32,
    pub parts: Vec<(String, Mesh)>,
}

pub struct CityImport {
    pub name: String,
    /// Папка города — оттуда же берётся готовый свет (`lights/*.alfar`).
    pub dir: PathBuf,
    pub groups: Vec<CityGroup>,
    pub materials: HashMap<String, Material>,
    pub bounds: (Vec3, Vec3),
    /// Высота земли в точке `ground_probe` (для точки спавна/камеры).
    pub ground_y: Option<f32>,
    pub triangles: u64,
}

fn load_chunk_into(path: &Path, per_mat: &mut HashMap<String, Accum>, probe: [f32; 2], best: &mut (f32, Option<f32>)) -> Result<()> {
    let file = std::fs::File::open(path).with_context(|| format!("не удалось открыть '{}'", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    let opts = tobj::LoadOptions { single_index: true, triangulate: true, ignore_points: true, ignore_lines: true };
    // .mtl уже разобран один раз в `load_materials` — здесь его не читаем
    let (models, mats) = tobj::load_obj_buf(&mut reader, &opts, |_| Err(tobj::LoadError::OpenFileFailed))
        .map_err(|e| anyhow!("'{}': {}", path.display(), e))?;
    let mats = mats.unwrap_or_default();
    for model in models {
        let mesh = model.mesh;
        let mat_name = mesh
            .material_id
            .and_then(|i| mats.get(i).map(|m| m.name.clone()))
            .unwrap_or_else(|| model.name.clone());
        let n = mesh.positions.len() / 3;
        if n == 0 {
            continue;
        }
        let acc = per_mat.entry(mat_name.clone()).or_default();
        let base = acc.vertices.len() as u32;
        // цвет вершин (фасады по снимкам): храним, только если он не белый
        let colored = mesh.vertex_color.len() >= n * 3 && mesh.vertex_color.iter().any(|&c| (c - 1.0).abs() > 1e-3);
        if colored && acc.colors.len() < acc.vertices.len() {
            acc.colors.resize(acc.vertices.len(), [1.0; 4]);
        }
        let ground = is_ground_material(&mat_name);
        for i in 0..n {
            let p = Vec3::new(mesh.positions[i * 3], mesh.positions[i * 3 + 1], mesh.positions[i * 3 + 2]);
            if ground {
                let d = (p.x - probe[0]).abs() + (p.z - probe[1]).abs();
                if d < best.0 {
                    *best = (d, Some(p.y));
                }
            }
            acc.vertices.push(p);
            acc.normals.push(if mesh.normals.len() >= (i + 1) * 3 {
                Vec3::new(mesh.normals[i * 3], mesh.normals[i * 3 + 1], mesh.normals[i * 3 + 2])
            } else {
                Vec3::UP
            });
            // та же конвенция, что `AssetLibrary::parse_obj`: V сверху вниз
            acc.uv.push(if mesh.texcoords.len() >= (i + 1) * 2 {
                [mesh.texcoords[i * 2], 1.0 - mesh.texcoords[i * 2 + 1]]
            } else {
                [0.0, 0.0]
            });
            if colored {
                acc.colors.push([mesh.vertex_color[i * 3], mesh.vertex_color[i * 3 + 1], mesh.vertex_color[i * 3 + 2], 1.0]);
            } else if !acc.colors.is_empty() {
                acc.colors.push([1.0; 4]);
            }
        }
        acc.indices.extend(mesh.indices.iter().map(|&i| i + base));
    }
    Ok(())
}

/// Читает выбранные чанки параллельно (rayon), сводит их в кварталы
/// `CHUNKS_PER_GROUP`x`CHUNKS_PER_GROUP` по материалам. `progress` —
/// счётчик прочитанных чанков для прогресс-бара UI.
pub fn import_city(
    info: &CityInfo,
    chunks: &[CityChunk],
    ground_probe: [f32; 2],
    progress: &AtomicUsize,
    log: &(dyn Fn(String) + Sync),
) -> Result<CityImport> {
    let materials = load_materials(&info.mtl_path, log)?;
    log(format!("🎨 Материалов: {} (albedo + normal + roughness)", materials.len()));

    let mut by_group: HashMap<(i32, i32), Vec<&CityChunk>> = HashMap::new();
    for c in chunks {
        by_group
            .entry((c.gx.div_euclid(CHUNKS_PER_GROUP), c.gz.div_euclid(CHUNKS_PER_GROUP)))
            .or_default()
            .push(c);
    }
    let mut keys: Vec<(i32, i32)> = by_group.keys().copied().collect();
    keys.sort();

    let results: Vec<Result<(CityGroup, (f32, Option<f32>))>> = keys
        .par_iter()
        .map(|key| {
            let mut per_mat: HashMap<String, Accum> = HashMap::new();
            let mut best = (f32::MAX, None);
            for c in &by_group[key] {
                load_chunk_into(&c.path, &mut per_mat, ground_probe, &mut best)?;
                progress.fetch_add(1, Ordering::Relaxed);
            }
            let mut parts: Vec<(String, Mesh)> = per_mat
                .into_iter()
                .filter(|(_, a)| !a.indices.is_empty())
                .map(|(m, a)| (m, a.into_mesh()))
                .collect();
            parts.sort_by(|a, b| a.0.cmp(&b.0));
            Ok((CityGroup { gx: key.0, gz: key.1, parts }, best))
        })
        .collect();

    let mut groups = Vec::with_capacity(results.len());
    let mut best = (f32::MAX, None);
    let mut min = Vec3::new(f32::MAX, f32::MAX, f32::MAX);
    let mut max = Vec3::new(f32::MIN, f32::MIN, f32::MIN);
    let mut triangles = 0u64;
    for r in results {
        let (g, b) = r?;
        if b.0 < best.0 {
            best = b;
        }
        for (_, m) in &g.parts {
            min = min.min(m.bounds.0);
            max = max.max(m.bounds.1);
            triangles += m.indices.len() as u64 / 3;
        }
        groups.push(g);
    }
    let missing: Vec<&String> = {
        let mut v: Vec<&String> = groups
            .iter()
            .flat_map(|g| g.parts.iter().map(|(m, _)| m))
            .filter(|m| !materials.contains_key(*m))
            .collect();
        v.sort();
        v.dedup();
        v
    };
    if !missing.is_empty() {
        log(format!("⚠️ В .mtl нет материалов: {:?} — будут серыми", missing));
    }
    Ok(CityImport { name: info.name.clone(), dir: info.dir.clone(), groups, materials, bounds: (min, max), ground_y: best.1, triangles })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{GameObject, MeshComponent, ObjectType, Scene};

    /// Сквозная проверка на реальной карте (tools/samara_map): импорт центра
    /// -> экспорт .alworld -> обратный импорт. Внешние данные, поэтому
    /// `#[ignore]`: `cargo test city_roundtrip -- --ignored`
    /// (папка — `SAMARA_MAP_DIR` или ../samara_map).
    #[test]
    #[ignore]
    fn city_roundtrip_keeps_textures_shared() {
        let dir = std::env::var("SAMARA_MAP_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../samara_map")));
        if !dir.join("chunks").is_dir() {
            eprintln!("нет {} — пропуск", dir.display());
            return;
        }
        let info = CityInfo::open(&dir).expect("папка города");
        let chunks = info.select([0.0, 0.0], Some(600.0));
        assert!(!chunks.is_empty());
        let progress = AtomicUsize::new(0);
        let city = import_city(&info, &chunks, [0.0, 0.0], &progress, &|m| eprintln!("{m}")).expect("импорт");
        assert_eq!(progress.load(Ordering::Relaxed), chunks.len());
        assert!(city.triangles > 10_000, "слишком мало треугольников: {}", city.triangles);
        assert!(city.ground_y.is_some());

        let asphalt = &city.materials["asphalt"];
        assert!(asphalt.albedo_texture.is_some() && asphalt.normal_texture.is_some() && asphalt.metallic_roughness_texture.is_some());
        let mr = asphalt.metallic_roughness_texture.as_ref().unwrap();
        assert_eq!(mr.pixels[0], 0, "R = metallic асфальта = 0");

        let mut scene = Scene::new("city");
        let mut mesh_objects = 0;
        for g in city.groups {
            for (mat, mesh) in g.parts {
                assert_eq!(mesh.uv.len(), mesh.vertices.len());
                let material = city.materials.get(&mat).cloned().unwrap_or_default();
                scene.add_object(GameObject::new(&mat, ObjectType::Mesh(MeshComponent {
                    mesh, material, visible: true, wireframe: false, solid: true, double_sided: false,
                })));
                mesh_objects += 1;
            }
        }
        let out = std::env::temp_dir().join(format!("alk_city_rt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&out);
        crate::converters::alworld::export_scene_to_alworld(&scene, out.to_str().unwrap()).expect("экспорт");

        let tex_files = std::fs::read_dir(out.join("textures")).unwrap().count();
        let used_maps: usize = city.materials.values().filter(|m| scene.objects.values().any(|o| matches!(&o.object_type, ObjectType::Mesh(mc) if mc.material.name == m.name)))
            .map(|m| m.albedo_texture.is_some() as usize + m.normal_texture.is_some() as usize + m.metallic_roughness_texture.is_some() as usize)
            .sum();
        assert_eq!(tex_files, used_maps, "каждая карта ровно одним файлом");

        // куски не несут пикселей, только ссылки
        let mut biggest_piece = 0u64;
        let mut pieces = 0usize;
        for e in std::fs::read_dir(out.join("objects")).unwrap() {
            let p = e.unwrap().path();
            biggest_piece = biggest_piece.max(std::fs::metadata(&p).unwrap().len());
            pieces += 1;
            let a = alkash3d_rs::altex_format::AltexFile::load(p.to_str().unwrap()).unwrap();
            assert!(a.texture_data.is_empty(), "{} содержит встроенные пиксели", p.display());
            if let Some(path) = a.extern_texture_path(a.materials[0].albedo_map) {
                assert!(std::path::Path::new(path).exists());
            }
        }
        eprintln!("объектов {mesh_objects}, кусков {pieces}, файлов текстур {tex_files}, крупнейший кусок {} КБ", biggest_piece / 1024);

        let reimported = crate::converters::alworld::import_alworld_to_scene(out.join("world.alworld").to_str().unwrap(), &mut |m| eprintln!("{m}"))
            .expect("обратный импорт");
        let textured = reimported.objects.values().filter(|o| matches!(&o.object_type,
            ObjectType::Mesh(m) if m.material.albedo_texture.is_some() && m.material.normal_texture.is_some())).count();
        assert!(textured > 0, "после обратного импорта текстуры потерялись");
        let _ = std::fs::remove_dir_all(&out);
    }
}
