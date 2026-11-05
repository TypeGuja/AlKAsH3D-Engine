//! Чтение моделей glTF 2.0 (.gltf с .bin и картинками рядом или внутри, .glb) в
//! данные на процессоре: треугольники с нормалями, UV и касательными, материалы
//! PBR (metallic-roughness), картинки в RGBA8. Без GPU — проверяется тестами;
//! в движок загружает `AlkashEngine::load_gltf`.
//!
//! Поддержано: сцена и иерархия узлов (TRS или матрица — запекаются в вершины),
//! треугольные примитивы, индексы 8/16/32 бит, нормализованные атрибуты, byteStride,
//! base64 и внешние файлы, PNG и JPEG, alphaMode MASK/BLEND, emissiveFactor.
//! Не поддержано (явная ошибка, а не тихо криво): сжатие Draco/meshopt, sparse-аксессоры,
//! KHR_texture_transform. Анимации и скелеты пропускаются: модель берётся в позе покоя.

use glam::{Mat3, Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// JSON

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(a) => a,
            _ => &[],
        }
    }
    pub fn num(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn usize_at(&self, key: &str) -> Option<usize> {
        self.get(key).and_then(Json::num).map(|n| n as usize)
    }
    fn f32s(&self, key: &str) -> Option<Vec<f32>> {
        self.get(key).map(|v| v.arr().iter().filter_map(Json::num).map(|n| n as f32).collect())
    }
}

pub fn parse_json(text: &str) -> Result<Json, String> {
    let mut p = JsonParser { s: text.as_bytes(), i: 0 };
    let v = p.value()?;
    p.ws();
    if p.i != p.s.len() {
        return Err(format!("JSON: лишнее после значения, позиция {}", p.i));
    }
    Ok(v)
}

struct JsonParser<'a> {
    s: &'a [u8],
    i: usize,
}

impl JsonParser<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!("JSON: {} в позиции {}", what, self.i))
    }
    fn value(&mut self) -> Result<Json, String> {
        self.ws();
        let Some(&c) = self.s.get(self.i) else { return self.err("неожиданный конец") };
        match c {
            b'{' => {
                self.i += 1;
                let mut fields = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Json::Obj(fields));
                }
                loop {
                    self.ws();
                    let key = match self.value()? {
                        Json::Str(k) => k,
                        _ => return self.err("ключ объекта не строка"),
                    };
                    self.ws();
                    if self.s.get(self.i) != Some(&b':') {
                        return self.err("нет двоеточия");
                    }
                    self.i += 1;
                    let v = self.value()?;
                    fields.push((key, v));
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Json::Obj(fields));
                        }
                        _ => return self.err("ожидалась , или }"),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Json::Arr(items));
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Json::Arr(items));
                        }
                        _ => return self.err("ожидалась , или ]"),
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let mut out = String::new();
                loop {
                    let Some(&c) = self.s.get(self.i) else { return self.err("строка не закрыта") };
                    self.i += 1;
                    match c {
                        b'"' => return Ok(Json::Str(out)),
                        b'\\' => {
                            let Some(&e) = self.s.get(self.i) else { return self.err("обрыв экранирования") };
                            self.i += 1;
                            match e {
                                b'"' => out.push('"'),
                                b'\\' => out.push('\\'),
                                b'/' => out.push('/'),
                                b'b' => out.push('\u{8}'),
                                b'f' => out.push('\u{c}'),
                                b'n' => out.push('\n'),
                                b'r' => out.push('\r'),
                                b't' => out.push('\t'),
                                b'u' => {
                                    let mut code = self.hex4()?;
                                    // суррогатная пара
                                    if (0xD800..0xDC00).contains(&code) && self.s.get(self.i) == Some(&b'\\') && self.s.get(self.i + 1) == Some(&b'u') {
                                        self.i += 2;
                                        let low = self.hex4()?;
                                        code = 0x10000 + ((code - 0xD800) << 10) + (low - 0xDC00);
                                    }
                                    out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                                }
                                _ => return self.err("неизвестное экранирование"),
                            }
                        }
                        _ => {
                            // UTF-8 переносим как есть: начало многобайтной последовательности
                            let start = self.i - 1;
                            let len = match c {
                                0x00..=0x7F => 1,
                                0xC0..=0xDF => 2,
                                0xE0..=0xEF => 3,
                                _ => 4,
                            };
                            let end = (start + len).min(self.s.len());
                            out.push_str(&String::from_utf8_lossy(&self.s[start..end]));
                            self.i = end;
                        }
                    }
                }
            }
            b't' if self.s[self.i..].starts_with(b"true") => {
                self.i += 4;
                Ok(Json::Bool(true))
            }
            b'f' if self.s[self.i..].starts_with(b"false") => {
                self.i += 5;
                Ok(Json::Bool(false))
            }
            b'n' if self.s[self.i..].starts_with(b"null") => {
                self.i += 4;
                Ok(Json::Null)
            }
            _ => {
                let start = self.i;
                while self.i < self.s.len() && matches!(self.s[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                    self.i += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                text.parse::<f64>().map(Json::Num).or_else(|_| self.err("не число"))
            }
        }
    }
    fn hex4(&mut self) -> Result<u32, String> {
        let h = self.s.get(self.i..self.i + 4).ok_or("JSON: обрыв \\u")?;
        self.i += 4;
        u32::from_str_radix(std::str::from_utf8(h).unwrap_or(""), 16).map_err(|_| "JSON: плохой \\u".to_string())
    }
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return Err("base64: недопустимый символ".into()),
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// Модель

/// Картинка RGBA8, строки сверху вниз (как в файле и как UV glTF).
#[derive(Clone, Debug)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AlphaMode {
    Opaque,
    Mask(f32),
    Blend,
}

#[derive(Clone, Debug)]
pub struct Material {
    pub name: String,
    pub base_color: [f32; 4],
    pub base_color_texture: Option<usize>,
    pub metallic: f32,
    pub roughness: f32,
    /// glTF: G — шероховатость, B — металл (уже умноженные на коэффициенты нет: см. load).
    pub metallic_roughness_texture: Option<usize>,
    pub normal_texture: Option<usize>,
    pub normal_scale: f32,
    pub emissive: [f32; 3],
    pub alpha: AlphaMode,
    pub double_sided: bool,
}

impl Default for Material {
    fn default() -> Self {
        // материал по умолчанию из спецификации
        Material { name: String::new(), base_color: [1.0; 4], base_color_texture: None, metallic: 1.0, roughness: 1.0, metallic_roughness_texture: None, normal_texture: None, normal_scale: 1.0, emissive: [0.0; 3], alpha: AlphaMode::Opaque, double_sided: false }
    }
}

/// Вершина в координатах модели после всех преобразований узлов (метры, +Y вверх).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    /// xyz — касательная вдоль U, w — знак битангенса (как в glTF).
    pub tangent: [f32; 4],
    pub color: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Primitive {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    pub material: usize,
    /// Имя узла, из которого взят примитив.
    pub node: String,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub primitives: Vec<Primitive>,
    pub materials: Vec<Material>,
    /// Картинки по номерам текстур glTF (не картинок): `None` — текстура без картинки.
    pub textures: Vec<Option<Image>>,
    pub bounds_min: [f32; 3],
    pub bounds_max: [f32; 3],
}

const GLB_MAGIC: u32 = 0x4654_6C67;
const CHUNK_JSON: u32 = 0x4E4F_534A;
const CHUNK_BIN: u32 = 0x004E_4942;

/// Прочитать .gltf или .glb.
pub fn load(path: &Path) -> Result<Model, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_default();
    load_bytes(&bytes, &dir)
}

/// Разобрать glTF из памяти; внешние файлы ищутся в `dir`.
pub fn load_bytes(bytes: &[u8], dir: &Path) -> Result<Model, String> {
    let (json_text, bin) = if bytes.len() >= 12 && u32::from_le_bytes(bytes[0..4].try_into().unwrap()) == GLB_MAGIC {
        split_glb(bytes)?
    } else {
        (std::str::from_utf8(bytes).map_err(|_| "glTF: не UTF-8 и не GLB")?.to_string(), None)
    };
    let doc = parse_json(&json_text)?;
    Loader { doc: &doc, dir: dir.to_path_buf(), glb_bin: bin, buffers: HashMap::new() }.run()
}

fn split_glb(bytes: &[u8]) -> Result<(String, Option<Vec<u8>>), String> {
    let version = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    if version != 2 {
        return Err(format!("GLB: версия {}, нужна 2", version));
    }
    let total = (u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize).min(bytes.len());
    let (mut at, mut json, mut bin) = (12usize, None, None);
    while at + 8 <= total {
        let len = u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap());
        let data = bytes.get(at + 8..at + 8 + len).ok_or("GLB: блок за концом файла")?;
        match kind {
            CHUNK_JSON => json = Some(String::from_utf8_lossy(data).into_owned()),
            CHUNK_BIN if bin.is_none() => bin = Some(data.to_vec()),
            _ => {}
        }
        at += 8 + len;
    }
    Ok((json.ok_or("GLB: нет блока JSON")?, bin))
}

struct Loader<'a> {
    doc: &'a Json,
    dir: PathBuf,
    glb_bin: Option<Vec<u8>>,
    buffers: HashMap<usize, Vec<u8>>,
}

impl Loader<'_> {
    fn run(mut self) -> Result<Model, String> {
        for ext in self.doc.get("extensionsRequired").map(Json::arr).unwrap_or(&[]) {
            let name = ext.str().unwrap_or("?");
            return Err(format!("glTF: нужно расширение {}, оно не поддержано", name));
        }
        let mut model = Model { bounds_min: [f32::MAX; 3], bounds_max: [f32::MIN; 3], ..Default::default() };
        model.materials = self.doc.get("materials").map(Json::arr).unwrap_or(&[]).iter().map(read_material).collect();
        model.textures = (0..self.doc.get("textures").map(Json::arr).unwrap_or(&[]).len()).map(|t| self.texture(t)).collect::<Result<_, _>>()?;

        let scene = self.doc.usize_at("scene").unwrap_or(0);
        let roots: Vec<usize> = match self.doc.get("scenes").map(Json::arr).and_then(|s| s.get(scene)) {
            Some(s) => s.get("nodes").map(Json::arr).unwrap_or(&[]).iter().filter_map(Json::num).map(|n| n as usize).collect(),
            // без сцен — все узлы, у которых нет родителя
            None => {
                let nodes = self.doc.get("nodes").map(Json::arr).unwrap_or(&[]);
                let children: Vec<usize> = nodes.iter().flat_map(|n| n.get("children").map(Json::arr).unwrap_or(&[]).iter().filter_map(Json::num).map(|c| c as usize)).collect();
                (0..nodes.len()).filter(|i| !children.contains(i)).collect()
            }
        };
        for r in roots {
            self.node(r, Mat4::IDENTITY, &mut model, 0)?;
        }
        if model.primitives.is_empty() {
            model.bounds_min = [0.0; 3];
            model.bounds_max = [0.0; 3];
        }
        Ok(model)
    }

    fn node(&mut self, index: usize, parent: Mat4, model: &mut Model, depth: usize) -> Result<(), String> {
        if depth > 64 {
            return Err("glTF: узлы зациклены".into());
        }
        let node = self.doc.get("nodes").map(Json::arr).and_then(|n| n.get(index)).ok_or(format!("glTF: нет узла {}", index))?;
        let local = if let Some(m) = node.f32s("matrix").filter(|m| m.len() == 16) {
            Mat4::from_cols_slice(&m)
        } else {
            let t = node.f32s("translation").filter(|v| v.len() == 3).map(|v| Vec3::new(v[0], v[1], v[2])).unwrap_or(Vec3::ZERO);
            let r = node.f32s("rotation").filter(|v| v.len() == 4).map(|v| Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize()).unwrap_or(Quat::IDENTITY);
            let s = node.f32s("scale").filter(|v| v.len() == 3).map(|v| Vec3::new(v[0], v[1], v[2])).unwrap_or(Vec3::ONE);
            Mat4::from_scale_rotation_translation(s, r, t)
        };
        let world = parent * local;
        if let Some(mesh) = node.usize_at("mesh") {
            let name = node.get("name").and_then(Json::str).unwrap_or("").to_string();
            self.mesh(mesh, world, &name, model)?;
        }
        let children: Vec<usize> = node.get("children").map(Json::arr).unwrap_or(&[]).iter().filter_map(Json::num).map(|c| c as usize).collect();
        for c in children {
            self.node(c, world, model, depth + 1)?;
        }
        Ok(())
    }

    fn mesh(&mut self, index: usize, world: Mat4, node_name: &str, model: &mut Model) -> Result<(), String> {
        let mesh = self.doc.get("meshes").map(Json::arr).and_then(|m| m.get(index)).ok_or(format!("glTF: нет меша {}", index))?;
        for prim in mesh.get("primitives").map(Json::arr).unwrap_or(&[]) {
            let mode = prim.usize_at("mode").unwrap_or(4);
            if mode != 4 {
                // точки, линии и полосы треугольников в модели обстановки не встречаются
                continue;
            }
            let attrs = prim.get("attributes").ok_or("glTF: примитив без атрибутов")?;
            let positions = self.accessor_f32(attrs.usize_at("POSITION").ok_or("glTF: нет POSITION")?)?;
            let n = positions.len();
            let normals = attrs.usize_at("NORMAL").map(|a| self.accessor_f32(a)).transpose()?;
            let uvs = attrs.usize_at("TEXCOORD_0").map(|a| self.accessor_f32(a)).transpose()?;
            let tangents = attrs.usize_at("TANGENT").map(|a| self.accessor_f32(a)).transpose()?;
            let colors = attrs.usize_at("COLOR_0").map(|a| self.accessor_f32(a)).transpose()?;
            let indices: Vec<u32> = match prim.usize_at("indices") {
                Some(a) => self.accessor_u32(a)?,
                None => (0..n as u32).collect(),
            };
            if indices.iter().any(|&i| i as usize >= n) {
                return Err("glTF: индекс за пределами вершин".into());
            }
            let material = prim.usize_at("material").unwrap_or(usize::MAX);
            let material = if material < model.materials.len() {
                material
            } else {
                model.materials.push(Material::default());
                model.materials.len() - 1
            };

            // нормали в мир — обратно-транспонированной; зеркальная матрица меняет порядок обхода
            let normal_m = Mat3::from_mat4(world).inverse().transpose();
            let mirrored = Mat3::from_mat4(world).determinant() < 0.0;
            let mut vertices: Vec<Vertex> = (0..n)
                .map(|i| {
                    let p = world.transform_point3(Vec3::new(positions[i][0], positions[i][1], positions[i][2]));
                    let nrm = normals.as_ref().map(|v| (normal_m * Vec3::new(v[i][0], v[i][1], v[i][2])).normalize_or_zero()).unwrap_or(Vec3::ZERO);
                    let uv = uvs.as_ref().map(|v| [v[i][0], v[i][1]]).unwrap_or([0.0, 0.0]);
                    let tangent = tangents.as_ref().map(|v| {
                        let t = (Mat3::from_mat4(world) * Vec3::new(v[i][0], v[i][1], v[i][2])).normalize_or_zero();
                        let w = if v[i].len() > 3 { v[i][3] } else { 1.0 };
                        [t.x, t.y, t.z, if mirrored { -w } else { w }]
                    }).unwrap_or([0.0; 4]);
                    let color = colors.as_ref().map(|v| [v[i][0], v[i][1], v[i][2], if v[i].len() > 3 { v[i][3] } else { 1.0 }]).unwrap_or([1.0; 4]);
                    Vertex { position: p.to_array(), normal: nrm.to_array(), uv, tangent, color }
                })
                .collect();
            let mut indices = indices;
            if mirrored {
                for t in indices.chunks_exact_mut(3) {
                    t.swap(1, 2);
                }
            }
            if normals.is_none() {
                compute_normals(&mut vertices, &indices);
            }
            if tangents.is_none() {
                compute_tangents(&mut vertices, &indices);
            }
            for v in &vertices {
                for k in 0..3 {
                    model.bounds_min[k] = model.bounds_min[k].min(v.position[k]);
                    model.bounds_max[k] = model.bounds_max[k].max(v.position[k]);
                }
            }
            model.primitives.push(Primitive { vertices, indices, material, node: node_name.to_string() });
        }
        Ok(())
    }

    fn buffer(&mut self, index: usize) -> Result<&Vec<u8>, String> {
        if !self.buffers.contains_key(&index) {
            let b = self.doc.get("buffers").map(Json::arr).and_then(|b| b.get(index)).ok_or(format!("glTF: нет буфера {}", index))?;
            let data = match b.get("uri").and_then(Json::str) {
                Some(uri) => self.uri(uri)?,
                None => self.glb_bin.clone().ok_or("glTF: буфер без uri, а блока BIN нет")?,
            };
            self.buffers.insert(index, data);
        }
        Ok(&self.buffers[&index])
    }

    fn uri(&self, uri: &str) -> Result<Vec<u8>, String> {
        if let Some(rest) = uri.strip_prefix("data:") {
            let comma = rest.find(',').ok_or("glTF: data URI без запятой")?;
            if !rest[..comma].ends_with(";base64") {
                return Err("glTF: data URI не base64".into());
            }
            return base64_decode(&rest[comma + 1..]);
        }
        let path = self.dir.join(percent_decode(uri));
        std::fs::read(&path).map_err(|e| format!("glTF: {}: {}", path.display(), e))
    }

    fn buffer_view(&mut self, index: usize) -> Result<(Vec<u8>, usize), String> {
        let view = self.doc.get("bufferViews").map(Json::arr).and_then(|v| v.get(index)).ok_or(format!("glTF: нет bufferView {}", index))?;
        let buffer = view.usize_at("buffer").ok_or("glTF: bufferView без buffer")?;
        let offset = view.usize_at("byteOffset").unwrap_or(0);
        let length = view.usize_at("byteLength").ok_or("glTF: bufferView без byteLength")?;
        let stride = view.usize_at("byteStride").unwrap_or(0);
        let data = self.buffer(buffer)?;
        let slice = data.get(offset..offset + length).ok_or("glTF: bufferView за концом буфера")?;
        Ok((slice.to_vec(), stride))
    }

    /// Аксессор как float-векторы: нормализованные целые — в 0..1 / -1..1, индексы — как есть.
    fn accessor_f32(&mut self, index: usize) -> Result<Vec<Vec<f32>>, String> {
        let acc = self.doc.get("accessors").map(Json::arr).and_then(|a| a.get(index)).ok_or(format!("glTF: нет аксессора {}", index))?.clone();
        if acc.get("sparse").is_some() {
            return Err("glTF: sparse-аксессоры не поддержаны".into());
        }
        let count = acc.usize_at("count").ok_or("glTF: аксессор без count")?;
        let width = match acc.get("type").and_then(Json::str) {
            Some("SCALAR") => 1,
            Some("VEC2") => 2,
            Some("VEC3") => 3,
            Some("VEC4") => 4,
            Some("MAT4") => 16,
            t => return Err(format!("glTF: тип аксессора {:?} не поддержан", t)),
        };
        let component = acc.usize_at("componentType").ok_or("glTF: аксессор без componentType")?;
        let size = match component {
            5120 | 5121 => 1,
            5122 | 5123 => 2,
            5125 | 5126 => 4,
            c => return Err(format!("glTF: componentType {} не поддержан", c)),
        };
        let normalized = acc.get("normalized") == Some(&Json::Bool(true));
        let Some(view) = acc.usize_at("bufferView") else {
            // без bufferView — нули (так в спецификации)
            return Ok(vec![vec![0.0; width]; count]);
        };
        let (data, stride) = self.buffer_view(view)?;
        let offset = acc.usize_at("byteOffset").unwrap_or(0);
        let stride = if stride == 0 { width * size } else { stride };
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let base = offset + i * stride;
            let mut v = Vec::with_capacity(width);
            for k in 0..width {
                let at = base + k * size;
                let b = data.get(at..at + size).ok_or("glTF: аксессор за концом bufferView")?;
                let x = match component {
                    5120 => {
                        let x = b[0] as i8 as f32;
                        if normalized { (x / 127.0).max(-1.0) } else { x }
                    }
                    5121 => {
                        let x = b[0] as f32;
                        if normalized { x / 255.0 } else { x }
                    }
                    5122 => {
                        let x = i16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized { (x / 32767.0).max(-1.0) } else { x }
                    }
                    5123 => {
                        let x = u16::from_le_bytes([b[0], b[1]]) as f32;
                        if normalized { x / 65535.0 } else { x }
                    }
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                    _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                };
                v.push(x);
            }
            out.push(v);
        }
        Ok(out)
    }

    /// Индексы: целые без потерь (u32 не помещается в f32 точно).
    fn accessor_u32(&mut self, index: usize) -> Result<Vec<u32>, String> {
        let acc = self.doc.get("accessors").map(Json::arr).and_then(|a| a.get(index)).ok_or(format!("glTF: нет аксессора {}", index))?.clone();
        let count = acc.usize_at("count").ok_or("glTF: аксессор без count")?;
        let size = match acc.usize_at("componentType") {
            Some(5121) => 1,
            Some(5123) => 2,
            Some(5125) => 4,
            c => return Err(format!("glTF: индексы типа {:?} не поддержаны", c)),
        };
        let view = acc.usize_at("bufferView").ok_or("glTF: индексы без bufferView")?;
        let (data, stride) = self.buffer_view(view)?;
        let offset = acc.usize_at("byteOffset").unwrap_or(0);
        let stride = if stride == 0 { size } else { stride };
        (0..count)
            .map(|i| {
                let at = offset + i * stride;
                let b = data.get(at..at + size).ok_or("glTF: индексы за концом bufferView")?;
                Ok(match size {
                    1 => b[0] as u32,
                    2 => u16::from_le_bytes([b[0], b[1]]) as u32,
                    _ => u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                })
            })
            .collect()
    }

    fn texture(&mut self, index: usize) -> Result<Option<Image>, String> {
        let tex = self.doc.get("textures").map(Json::arr).and_then(|t| t.get(index)).cloned().unwrap_or(Json::Null);
        let Some(source) = tex.usize_at("source") else { return Ok(None) };
        let image = self.doc.get("images").map(Json::arr).and_then(|i| i.get(source)).cloned().ok_or(format!("glTF: нет картинки {}", source))?;
        let bytes = if let Some(view) = image.usize_at("bufferView") {
            self.buffer_view(view)?.0
        } else if let Some(uri) = image.get("uri").and_then(Json::str) {
            self.uri(uri)?
        } else {
            return Err("glTF: картинка без данных".into());
        };
        decode_image(&bytes).map(Some)
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// PNG или JPEG в RGBA8.
pub fn decode_image(bytes: &[u8]) -> Result<Image, String> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::normalize_to_color8());
        let mut r = dec.read_info().map_err(|e| format!("PNG: {}", e))?;
        let mut buf = vec![0; r.output_buffer_size()];
        let info = r.next_frame(&mut buf).map_err(|e| format!("PNG: {}", e))?;
        let ch = info.color_type.samples();
        let (w, h) = (info.width, info.height);
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for p in buf[..(w * h) as usize * ch].chunks_exact(ch) {
            match ch {
                1 => rgba.extend([p[0], p[0], p[0], 255]),
                2 => rgba.extend([p[0], p[0], p[0], p[1]]),
                3 => rgba.extend([p[0], p[1], p[2], 255]),
                _ => rgba.extend([p[0], p[1], p[2], p[3]]),
            }
        }
        return Ok(Image { width: w, height: h, rgba });
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        let pixels = dec.decode().map_err(|e| format!("JPEG: {}", e))?;
        let info = dec.info().ok_or("JPEG: нет заголовка")?;
        let (w, h) = (info.width as u32, info.height as u32);
        let rgba = match info.pixel_format {
            jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&l| [l, l, l, 255]).collect(),
            jpeg_decoder::PixelFormat::RGB24 => pixels.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            f => return Err(format!("JPEG: формат {:?} не поддержан", f)),
        };
        return Ok(Image { width: w, height: h, rgba });
    }
    Err("картинка не PNG и не JPEG".into())
}

fn read_material(m: &Json) -> Material {
    let mut out = Material { name: m.get("name").and_then(Json::str).unwrap_or("").to_string(), ..Default::default() };
    let tex = |v: Option<&Json>| -> Option<usize> {
        let v = v?;
        if v.get("extensions").and_then(|e| e.get("KHR_texture_transform")).is_some() {
            eprintln!("[glTF] KHR_texture_transform пропущен: текстура может лечь со сдвигом");
        }
        v.usize_at("index")
    };
    if let Some(pbr) = m.get("pbrMetallicRoughness") {
        if let Some(c) = pbr.f32s("baseColorFactor").filter(|c| c.len() == 4) {
            out.base_color = [c[0], c[1], c[2], c[3]];
        }
        out.base_color_texture = tex(pbr.get("baseColorTexture"));
        out.metallic = pbr.get("metallicFactor").and_then(Json::num).unwrap_or(1.0) as f32;
        out.roughness = pbr.get("roughnessFactor").and_then(Json::num).unwrap_or(1.0) as f32;
        out.metallic_roughness_texture = tex(pbr.get("metallicRoughnessTexture"));
    }
    if let Some(n) = m.get("normalTexture") {
        out.normal_texture = tex(Some(n));
        out.normal_scale = n.get("scale").and_then(Json::num).unwrap_or(1.0) as f32;
    }
    if let Some(e) = m.f32s("emissiveFactor").filter(|e| e.len() == 3) {
        out.emissive = [e[0], e[1], e[2]];
    }
    out.alpha = match m.get("alphaMode").and_then(Json::str) {
        Some("MASK") => AlphaMode::Mask(m.get("alphaCutoff").and_then(Json::num).unwrap_or(0.5) as f32),
        Some("BLEND") => AlphaMode::Blend,
        _ => AlphaMode::Opaque,
    };
    out.double_sided = m.get("doubleSided") == Some(&Json::Bool(true));
    out
}

/// Нормали по треугольникам, если в файле их нет (сглаженные по общим вершинам).
fn compute_normals(v: &mut [Vertex], idx: &[u32]) {
    let mut acc = vec![Vec3::ZERO; v.len()];
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (Vec3::from(v[t[0] as usize].position), Vec3::from(v[t[1] as usize].position), Vec3::from(v[t[2] as usize].position));
        let n = (b - a).cross(c - a);
        for &i in t {
            acc[i as usize] += n;
        }
    }
    for (vert, n) in v.iter_mut().zip(acc) {
        vert.normal = n.normalize_or(Vec3::Y).to_array();
    }
}

/// Касательные по UV (Lengyel), если в файле их нет: нужны для карт нормалей.
fn compute_tangents(v: &mut [Vertex], idx: &[u32]) {
    let mut tan = vec![Vec3::ZERO; v.len()];
    let mut bit = vec![Vec3::ZERO; v.len()];
    for t in idx.chunks_exact(3) {
        let (i0, i1, i2) = (t[0] as usize, t[1] as usize, t[2] as usize);
        let (p0, p1, p2) = (Vec3::from(v[i0].position), Vec3::from(v[i1].position), Vec3::from(v[i2].position));
        let (u0, u1, u2) = (v[i0].uv, v[i1].uv, v[i2].uv);
        let (e1, e2) = (p1 - p0, p2 - p0);
        let (du1, dv1, du2, dv2) = (u1[0] - u0[0], u1[1] - u0[1], u2[0] - u0[0], u2[1] - u0[1]);
        let det = du1 * dv2 - du2 * dv1;
        if det.abs() < 1e-12 {
            continue;
        }
        let r = 1.0 / det;
        let sdir = (e1 * dv2 - e2 * dv1) * r;
        let tdir = (e2 * du1 - e1 * du2) * r;
        for i in [i0, i1, i2] {
            tan[i] += sdir;
            bit[i] += tdir;
        }
    }
    for i in 0..v.len() {
        let n = Vec3::from(v[i].normal);
        let t = tan[i] - n * n.dot(tan[i]);
        let t = if t.length_squared() > 1e-20 { t.normalize() } else { n.any_orthonormal_vector() };
        // UV glTF растут вниз по картинке, поэтому битангенс по glTF — к +V
        let w = if n.cross(t).dot(bit[i]) < 0.0 { -1.0 } else { 1.0 };
        v[i].tangent = [t.x, t.y, t.z, w];
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Куб 1 м: GLB с одним узлом (сдвиг и поворот), без нормалей и UV в файле.
    pub(crate) fn cube_glb(translation: [f32; 3]) -> Vec<u8> {
        cube_glb_with(translation, r#""rotation":[0,0.7071068,0,0.7071068]"#, "")
    }

    /// `transform` — поля поворота/масштаба узла, `top` — поля в начало документа.
    pub(crate) fn cube_glb_with(translation: [f32; 3], transform: &str, top: &str) -> Vec<u8> {
        let p: [[f32; 3]; 8] = [[-0.5, -0.5, -0.5], [0.5, -0.5, -0.5], [0.5, 0.5, -0.5], [-0.5, 0.5, -0.5], [-0.5, -0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, 0.5], [-0.5, 0.5, 0.5]];
        let idx: [u16; 36] = [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 3, 7, 6, 3, 6, 2, 0, 4, 7, 0, 7, 3, 1, 2, 6, 1, 6, 5];
        let mut bin: Vec<u8> = Vec::new();
        for v in p {
            for c in v {
                bin.extend(c.to_le_bytes());
            }
        }
        for i in idx {
            bin.extend(i.to_le_bytes());
        }
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let json = format!(
            r#"{{{}"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"name":"Ящик","mesh":0,"translation":[{},{},{}],{}}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
            "materials":[{{"name":"краска","pbrMetallicRoughness":{{"baseColorFactor":[0.5,0.2,0.1,1],"metallicFactor":0,"roughnessFactor":0.6}},"alphaMode":"MASK","alphaCutoff":0.3}}],
            "accessors":[{{"bufferView":0,"componentType":5126,"count":8,"type":"VEC3","min":[-0.5,-0.5,-0.5],"max":[0.5,0.5,0.5]}},
                         {{"bufferView":1,"componentType":5123,"count":36,"type":"SCALAR"}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":96}},{{"buffer":0,"byteOffset":96,"byteLength":72}}],
            "buffers":[{{"byteLength":{}}}]}}"#,
            top, translation[0], translation[1], translation[2], transform, bin.len()
        );
        let mut json = json.into_bytes();
        while json.len() % 4 != 0 {
            json.push(b' ');
        }
        let mut out = Vec::new();
        out.extend(GLB_MAGIC.to_le_bytes());
        out.extend(2u32.to_le_bytes());
        out.extend(((12 + 8 + json.len() + 8 + bin.len()) as u32).to_le_bytes());
        out.extend((json.len() as u32).to_le_bytes());
        out.extend(CHUNK_JSON.to_le_bytes());
        out.extend(&json);
        out.extend((bin.len() as u32).to_le_bytes());
        out.extend(CHUNK_BIN.to_le_bytes());
        out.extend(&bin);
        out
    }

    #[test]
    fn json_parses_nested_values_and_escapes() {
        let v = parse_json(r#"{"a":[1,-2.5e1,true,null],"b":{"c":"до\"мA"},"d":[]}"#).unwrap();
        assert_eq!(v.get("a").unwrap().arr()[1].num(), Some(-25.0));
        assert_eq!(v.get("b").unwrap().get("c").unwrap().str(), Some("до\"мA"));
        assert!(v.get("d").unwrap().arr().is_empty());
        assert!(parse_json("{\"a\":}").is_err());
        assert!(parse_json("[1,2").is_err());
    }

    #[test]
    fn base64_round_trip() {
        assert_eq!(base64_decode("0J/RgNC40LLQtdGC").unwrap(), "Привет".as_bytes());
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
    }

    #[test]
    fn glb_cube_is_read_with_transform_material_and_normals() {
        let m = load_bytes(&cube_glb([1.0, 2.0, 3.0]), Path::new(".")).unwrap();
        assert_eq!(m.primitives.len(), 1);
        let p = &m.primitives[0];
        assert_eq!(p.indices.len(), 36);
        assert_eq!(p.node, "Ящик");
        // поворот на 90° вокруг Y не меняет куб, сдвиг — да
        for k in 0..3 {
            assert!((m.bounds_min[k] - ([1.0, 2.0, 3.0][k] - 0.5)).abs() < 1e-5);
            assert!((m.bounds_max[k] - ([1.0, 2.0, 3.0][k] + 0.5)).abs() < 1e-5);
        }
        // нормали посчитаны и смотрят наружу от центра
        let c = Vec3::new(1.0, 2.0, 3.0);
        for v in &p.vertices {
            let n = Vec3::from(v.normal);
            assert!((n.length() - 1.0).abs() < 1e-4);
            assert!(n.dot(Vec3::from(v.position) - c) > 0.0, "нормаль внутрь");
        }
        let mat = &m.materials[p.material];
        assert_eq!(mat.name, "краска");
        assert_eq!(mat.base_color, [0.5, 0.2, 0.1, 1.0]);
        assert_eq!((mat.metallic, mat.roughness), (0.0, 0.6));
        assert_eq!(mat.alpha, AlphaMode::Mask(0.3));
    }

    #[test]
    fn mirrored_node_keeps_triangles_facing_out() {
        let glb = cube_glb_with([0.0, 0.0, 0.0], r#""scale":[-1,1,1]"#, "");
        let m = load_bytes(&glb, Path::new(".")).unwrap();
        let p = &m.primitives[0];
        for t in p.indices.chunks_exact(3) {
            let (a, b, c) = (Vec3::from(p.vertices[t[0] as usize].position), Vec3::from(p.vertices[t[1] as usize].position), Vec3::from(p.vertices[t[2] as usize].position));
            let n = (b - a).cross(c - a);
            assert!(n.dot((a + b + c) / 3.0) > 0.0, "треугольник вывернут");
        }
    }

    #[test]
    fn tangents_follow_the_uv_layout() {
        // квадрат в плоскости XY, U вдоль +X, V вдоль -Y (как в glTF: V вниз по картинке)
        let mut v = vec![
            Vertex { position: [0.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0], uv: [0.0, 1.0], tangent: [0.0; 4], color: [1.0; 4] },
            Vertex { position: [1.0, 0.0, 0.0], normal: [0.0, 0.0, 1.0], uv: [1.0, 1.0], tangent: [0.0; 4], color: [1.0; 4] },
            Vertex { position: [1.0, 1.0, 0.0], normal: [0.0, 0.0, 1.0], uv: [1.0, 0.0], tangent: [0.0; 4], color: [1.0; 4] },
        ];
        compute_tangents(&mut v, &[0, 1, 2]);
        for vert in &v {
            assert!((Vec3::from_slice(&vert.tangent[..3]) - Vec3::X).length() < 1e-5);
            // битангенс glTF = cross(N, T) * w смотрит к +V, то есть к -Y
            let b = Vec3::Z.cross(Vec3::X) * vert.tangent[3];
            assert!((b - Vec3::new(0.0, -1.0, 0.0)).length() < 1e-5, "битангенс {:?}", b);
        }
    }

    #[test]
    fn unsupported_compression_is_an_error_not_garbage() {
        let glb = cube_glb_with([0.0; 3], r#""scale":[1,1,1]"#, r#""extensionsRequired":["KHR_draco_mesh_compression"],"#);
        let err = load_bytes(&glb, Path::new(".")).unwrap_err();
        assert!(err.contains("KHR_draco"), "{}", err);
    }
}
