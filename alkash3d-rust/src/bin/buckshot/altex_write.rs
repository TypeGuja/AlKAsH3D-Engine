//! Запись сцены Buckshot (`SceneFile`) в форматы движка: `.altex` v3 с
//! блоком сцены + `.alfar` со светом; сверка перечитанного файла со входом.
//! Общий для `brsc2altex` и `void_scene`.

use alkash3d_rs::altex_format::{AltexFile, Transform, Vertex};
use alkash3d_rs::altex_scene::*;
use alkash3d_rs::math::{Mat4, Vec3};
use alkash3d_rs::{AlfarFile, IndividualLight, LightFalloff, LightType};
use crate::scene_file::{Keys, Kind, SceneFile, Value};

/// DXGI_FORMAT_R8G8B8A8_UNORM — как у экспорта эдитора.
const RGBA8: u32 = 28;
const NONE: u32 = 0xFFFF_FFFF;

fn xf12(m: &Mat4) -> Xform {
    let (x, y, z, w) = (m.x_axis, m.y_axis, m.z_axis, m.w_axis);
    [x.x, x.y, x.z, y.x, y.y, y.z, z.x, z.y, z.z, w.x, w.y, w.z]
}

fn idx(i: Option<usize>) -> i32 {
    i.map_or(-1, |i| i as i32)
}

fn value(v: &Value) -> AnimValue {
    match v {
        Value::None => AnimValue::None,
        Value::Float(f) => AnimValue::Float(*f),
        Value::Vec3(a) => AnimValue::Vec3(a.to_array()),
        Value::Quat(q) => AnimValue::Quat(q.to_array()),
        Value::Color(c) => AnimValue::Color(*c),
        Value::Xf(m) => AnimValue::Xf(xf12(m)),
        Value::Str(s) => AnimValue::Str(s.clone()),
        Value::Vec2(a) => AnimValue::Vec2(*a),
    }
}

/// Мировые матрицы узлов (top_level — без родителя, как в Godot).
fn globals(scene: &SceneFile) -> Vec<Mat4> {
    let mut g: Vec<Mat4> = Vec::with_capacity(scene.nodes.len());
    for n in &scene.nodes {
        let parent = n.parent.filter(|_| !n.top_level).map(|p| g[p]).unwrap_or(Mat4::IDENTITY);
        g.push(parent * n.xf);
    }
    g
}

pub fn convert(scene: &SceneFile) -> (AltexFile, AlfarFile) {
    let mut altex = AltexFile::new();
    let mut ext = AltexScene::default();

    for (i, t) in scene.textures.iter().enumerate() {
        altex.add_texture(t.width, t.height, RGBA8, &t.rgba, &format!("tex{}", i));
    }

    for (i, m) in scene.materials.iter().enumerate() {
        let tex = |t: i32| if t < 0 { NONE } else { t as u32 };
        let id = altex.add_material(m.albedo, tex(m.albedo_tex), &format!("mat{}", i)) as usize;
        let am = &mut altex.materials[id];
        am.normal_map = tex(m.normal_tex);
        am.roughness = m.roughness;
        am.metallic = m.metallic;
        am.emissive = m.emission;
        ext.material_extras.push(MaterialExtra {
            transparency: m.transparency,
            unshaded: m.unshaded as u32,
            uv_scale: m.uv_scale,
            uv_offset: m.uv_offset,
        });
    }

    for (mi, mesh) in scene.meshes.iter().enumerate() {
        let first = altex.meshes.len() as u32;
        for (si, s) in mesh.surfaces.iter().enumerate() {
            let mut lo = [f32::MAX; 3];
            let mut hi = [f32::MIN; 3];
            let vertices: Vec<Vertex> = s
                .vertices
                .iter()
                .map(|v| {
                    for k in 0..3 {
                        lo[k] = lo[k].min(v[k]);
                        hi[k] = hi[k].max(v[k]);
                    }
                    let n = Vec3::new(v[3], v[4], v[5]);
                    let t = Vec3::new(v[8], v[9], v[10]);
                    // Вырожденная касательная (t ∥ n): cross = 0 и знак w
                    // потерялся бы, тогда в битангенс кладём n * w.
                    let c = n.cross(t);
                    let b = if c.length_squared() > 1e-12 { c * v[11] } else { n * v[11] };
                    Vertex {
                        position: [v[0], v[1], v[2]],
                        normal: n.to_array(),
                        tangent: t.to_array(),
                        bitangent: b.to_array(),
                        uv: [v[6], v[7]],
                        uv2: [0.0, 0.0],
                        color: [1.0; 4],
                    }
                })
                .collect();
            let id = altex.add_mesh(vertices, s.indices.clone(), &format!("mesh{}_{}", mi, si));
            if !s.vertices.is_empty() {
                altex.meshes[id as usize].min_bound = lo;
                altex.meshes[id as usize].max_bound = hi;
            }
            if let Some((bones, weights)) = &s.skin {
                ext.skins.push(MeshSkin { mesh: id, bones: bones.clone(), weights: weights.clone() });
            }
        }
        ext.mesh_groups.push(MeshGroup { first, count: mesh.surfaces.len() as u32 });
    }

    let g = globals(scene);
    let mut alfar = AlfarFile::new();
    for (i, n) in scene.nodes.iter().enumerate() {
        let path = scene.path(i);
        let mut light = -1;
        if let Some(l) = n.light {
            let (_, rot, pos) = g[i].to_scale_rotation_translation();
            light = alfar.add_light(
                IndividualLight {
                    id: 0,
                    name_id: 0,
                    light_type: if l.spot { LightType::Spot as u32 } else { LightType::Point as u32 },
                    position: pos.to_array(),
                    direction: (rot * -Vec3::Z).to_array(),
                    up: (rot * Vec3::Y).to_array(),
                    color: l.color,
                    intensity: l.energy,
                    range: l.range,
                    falloff_type: LightFalloff::Custom as u32,
                    falloff_custom: l.attenuation,
                    spot_inner_angle: l.angle,
                    spot_outer_angle: l.angle,
                    casts_shadows: l.shadow as u32,
                    shadow_bias: 0.0,
                    shadow_resolution: 0,
                    flicker_enabled: 0,
                    flicker_speed: 0.0,
                    flicker_intensity: 0.0,
                    enabled: n.visible as u32,
                    active_from: 0.0,
                    active_to: 24.0,
                    has_physics: 0,
                    breakable: 0,
                    health: 0.0,
                    custom_data_offset: 0,
                },
                &path,
            ) as i32;
        }

        if let Some(mg) = n.mesh {
            let (scale, rotation, position) = g[i].to_scale_rotation_translation();
            let group = ext.mesh_groups[mg];
            for si in 0..group.count {
                let o = altex.add_object(
                    group.first + si,
                    Transform { position: position.to_array(), rotation: rotation.to_array(), scale: scale.to_array() },
                    &path,
                ) as usize;
                let mat = n.materials.get(si as usize).copied().unwrap_or(-1);
                altex.objects[o].material_id = if mat < 0 { NONE } else { mat as u32 };
                altex.objects[o].flags = n.visible as u32;
            }
        }

        let kind = match n.kind {
            Kind::Node => NodeKind::Node,
            Kind::Spatial => NodeKind::Spatial,
            Kind::Mesh => NodeKind::Mesh,
            Kind::Light => NodeKind::Light,
            Kind::Camera => NodeKind::Camera,
            Kind::Canvas => NodeKind::Canvas,
            Kind::Audio => NodeKind::Audio,
            Kind::Skeleton => NodeKind::Skeleton,
            Kind::Label => NodeKind::Label,
        };
        ext.nodes.push(SceneNode {
            name: n.name.clone(),
            parent: idx(n.parent),
            kind,
            flags: if n.visible { NODE_VISIBLE } else { 0 } | if n.top_level { NODE_TOP_LEVEL } else { 0 },
            xf: xf12(&n.xf),
            mesh_group: idx(n.mesh),
            materials: n.materials.clone(),
            light,
            fov: n.fov,
            audio: n.audio.clone(),
            skeleton: idx(n.skeleton),
            binds: n.binds.iter().map(|(b, m)| (*b, xf12(m))).collect(),
            bones: n
                .bones
                .iter()
                .map(|b| Bone {
                    name: b.name.clone(),
                    parent: idx(b.parent),
                    rest: xf12(&b.rest),
                    pos: b.pos.to_array(),
                    rot: b.rot.to_array(),
                    scale: b.scale.to_array(),
                })
                .collect(),
            label: n.label.as_ref().map(|l| Label {
                text: l.text.clone(),
                font_size: l.font_size,
                pixel_size: l.pixel_size,
                color: l.color,
                align: l.align,
                billboard: l.billboard,
            }),
        });
    }

    for p in &scene.players {
        ext.players.push(AnimPlayer {
            node: p.node as i32,
            speed: p.speed,
            animations: p
                .animations
                .iter()
                .map(|a| Animation {
                    name: a.name.clone(),
                    length: a.length,
                    looped: a.looped,
                    tracks: a
                        .tracks
                        .iter()
                        .map(|t| Track {
                            target: idx(t.target),
                            path: t.path.clone(),
                            track_type: t.type_id,
                            interp: t.interp,
                            keys: match &t.keys {
                                Keys::Value(k) => TrackKeys::Value(k.iter().map(|(time, e, v)| (*time, *e, value(v))).collect()),
                                Keys::Method(k) => TrackKeys::Method(k.iter().map(|(time, m, args)| (*time, m.clone(), args.iter().map(value).collect())).collect()),
                                Keys::Audio(k) => TrackKeys::Audio(k.clone()),
                            },
                        })
                        .collect(),
                })
                .collect(),
        });
    }

    altex.scene = Some(ext);
    (altex, alfar)
}

/// Сверка перечитанной сцены со входом: всё, что читает игра.
pub fn compare(a: &SceneFile, b: &SceneFile) -> Result<(), String> {
    let eq = |what: &str, x: usize, y: usize| if x == y { Ok(()) } else { Err(format!("{}: {} != {}", what, x, y)) };
    eq("текстур", a.textures.len(), b.textures.len())?;
    eq("материалов", a.materials.len(), b.materials.len())?;
    eq("мешей", a.meshes.len(), b.meshes.len())?;
    eq("узлов", a.nodes.len(), b.nodes.len())?;
    eq("плееров", a.players.len(), b.players.len())?;
    for (x, y) in a.textures.iter().zip(&b.textures) {
        if x.rgba != y.rgba {
            return Err("пиксели текстуры расходятся".into());
        }
    }
    for (mi, (x, y)) in a.meshes.iter().zip(&b.meshes).enumerate() {
        eq("поверхностей", x.surfaces.len(), y.surfaces.len())?;
        for (s, t) in x.surfaces.iter().zip(&y.surfaces) {
            if s.indices != t.indices || s.skin.as_ref().map(|k| &k.0) != t.skin.as_ref().map(|k| &k.0) {
                return Err(format!("меш {}: индексы/скин расходятся", mi));
            }
            for (v, w) in s.vertices.iter().zip(&t.vertices) {
                if v.iter().zip(w).any(|(p, q)| (p - q).abs() > 1e-4) {
                    return Err(format!("меш {}: вершины расходятся {:?} / {:?}", mi, v, w));
                }
            }
        }
    }
    for (i, (x, y)) in a.nodes.iter().zip(&b.nodes).enumerate() {
        let same = x.name == y.name
            && x.parent == y.parent
            && x.kind == y.kind
            && x.visible == y.visible
            && x.mesh == y.mesh
            && x.materials == y.materials
            && x.xf == y.xf
            && x.bones.len() == y.bones.len()
            && x.binds.len() == y.binds.len()
            && x.light.map(|l| (l.spot, l.color, l.energy, l.range, l.angle, l.attenuation)) == y.light.map(|l| (l.spot, l.color, l.energy, l.range, l.angle, l.attenuation));
        if !same {
            return Err(format!("узел {} ({}) расходится", i, a.path(i)));
        }
    }
    for (x, y) in a.players.iter().zip(&b.players) {
        let tracks = |p: &crate::scene_file::Player| p.animations.iter().map(|a| (a.name.clone(), a.tracks.len())).collect::<Vec<_>>();
        if x.node != y.node || tracks(x) != tracks(y) {
            return Err("анимации расходятся".into());
        }
    }
    Ok(())
}

