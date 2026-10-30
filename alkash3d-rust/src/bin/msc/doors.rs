//! Двери на шарнирах (HingeJoint оригинала): двери машин, задние двери и
//! крышки багажников, ворота гаража. ЛКМ — открыть/закрыть, створка плавно
//! поворачивается вокруг оси шарнира до предела.
//! У дверей машин пределы в сцене нулевые (их выставлял скрипт) — для них
//! берётся 70° в ту сторону, где дверь уходит наружу от кузова.

use alkash3d_rs::math::{Quat, Vec3};

use alkash3d_rs::engine::Mesh;

use crate::physics::ray_box;
use crate::world::World;
use crate::world_file::Shape;

pub struct Door {
    pub node: usize,
    base: Quat,
    axis: Vec3,
    lo: f32,
    hi: f32,
    pub angle: f32,
    pub open: bool,
    /// Бокс створки в координатах узла (для взгляда мышью).
    bmin: Vec3,
    bmax: Vec3,
}

fn node_box(world: &World, meshes: &[Mesh], root: usize) -> Option<(Vec3, Vec3)> {
    let inv = world.global[root].inverse();
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let mut stack = vec![root];
    while let Some(i) = stack.pop() {
        let m = inv * world.global[i];
        // Видимые меши створки (у многих дверей коллайдер — сетка).
        for &mi in &world.nodes[i].meshes {
            let c = Vec3::from(meshes[mi].bounding_center);
            let r = meshes[mi].bounding_radius * 0.7;
            for k in 0..8 {
                let p = c + Vec3::new(if k & 1 == 0 { -r } else { r }, if k & 2 == 0 { -r } else { r }, if k & 4 == 0 { -r } else { r });
                let q = m.transform_point3(p);
                lo = lo.min(q);
                hi = hi.max(q);
            }
        }
        for c in &world.nodes[i].colliders {
            if c.trigger {
                continue;
            }
            let (a, b) = match c.shape {
                Shape::Box { center, size } => (center - size * 0.5, center + size * 0.5),
                Shape::Sphere { center, radius } => (center - Vec3::splat(radius), center + Vec3::splat(radius)),
                Shape::Capsule { center, height, radius, .. } => (center - Vec3::splat(height.max(2.0 * radius) * 0.5), center + Vec3::splat(height.max(2.0 * radius) * 0.5)),
                Shape::Mesh { .. } => continue,
            };
            for k in 0..8 {
                let p = Vec3::new(if k & 1 == 0 { a.x } else { b.x }, if k & 2 == 0 { a.y } else { b.y }, if k & 4 == 0 { a.z } else { b.z });
                let q = m.transform_point3(p);
                lo = lo.min(q);
                hi = hi.max(q);
            }
        }
        stack.extend_from_slice(&world.children[i]);
    }
    (lo.x <= hi.x).then_some((lo, hi))
}

pub struct Doors {
    pub list: Vec<Door>,
}

impl Doors {
    pub fn new(world: &World, meshes: &[Mesh]) -> Doors {
        let mut list = Vec::new();
        for i in 0..world.nodes.len() {
            let n = &world.nodes[i];
            let (Some(h), true) = (n.hinge, n.door) else { continue };
            let Some((bmin, bmax)) = node_box(world, meshes, i) else { continue };
            let axis = h.axis.normalize_or_zero();
            if axis == Vec3::ZERO {
                continue;
            }
            let (mut lo, mut hi) = (h.min, h.max);
            if (hi - lo).abs() < 1.0 {
                // Предел не задан: 70° наружу. "Наружу" — центр створки
                // отходит от центра машины (корня).
                let mut root = i;
                while let Some(p) = world.nodes[root].parent {
                    if p == 0 {
                        break;
                    }
                    root = p;
                }
                let car = world.global[root].w_axis.truncate();
                let c_local = (bmin + bmax) * 0.5;
                let g = world.global[i];
                let test = |deg: f32| {
                    let r = Quat::from_axis_angle(axis, deg.to_radians());
                    let p = g.transform_point3(r * c_local);
                    Vec3::new(p.x - car.x, 0.0, p.z - car.z).length()
                };
                if test(30.0) >= test(-30.0) {
                    (lo, hi) = (0.0, 70.0);
                } else {
                    (lo, hi) = (-70.0, 0.0);
                }
            }
            list.push(Door { node: i, base: n.local.rot, axis, lo, hi, angle: 0.0f32.clamp(lo, hi), open: false, bmin, bmax });
        }
        println!("[MSC] дверей на шарнирах: {}", list.len());
        Doors { list }
    }

    /// Дверь на луче: (индекс, расстояние).
    pub fn pick(&self, world: &World, o: Vec3, d: Vec3, max: f32) -> Option<(usize, f32)> {
        self.list
            .iter()
            .enumerate()
            .filter(|(_, door)| world.visible[door.node])
            .filter_map(|(k, door)| {
                let inv = world.global[door.node].inverse();
                let lo = inv.transform_point3(o);
                let ld = inv.transform_vector3(d);
                let len = ld.length();
                ray_box(lo, ld / len, door.bmin, door.bmax).map(|t| (k, t / len)).filter(|x| x.1 <= max)
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    pub fn toggle(&mut self, k: usize) {
        self.list[k].open = !self.list[k].open;
    }

    /// Анимация створок; true — какая-то дверь двигалась.
    pub fn update(&mut self, world: &mut World, dt: f32) -> bool {
        let mut moved = false;
        for d in &mut self.list {
            // Открыто — к дальнему от нуля пределу.
            let target = if d.open { if d.hi.abs() >= d.lo.abs() { d.hi } else { d.lo } } else { 0.0f32.clamp(d.lo, d.hi) };
            if (target - d.angle).abs() < 0.01 {
                continue;
            }
            let step = 160.0 * dt;
            d.angle += (target - d.angle).clamp(-step, step);
            world.nodes[d.node].local.rot = d.base * Quat::from_axis_angle(d.axis, d.angle.to_radians());
            world.update_subtree(d.node);
            moved = true;
        }
        moved
    }

    /// Вытолкнуть сферу игрока из закрытых/двигающихся створок.
    pub fn push_player(&self, world: &World, center: Vec3, radius: f32) -> Vec3 {
        let mut shift = Vec3::ZERO;
        for d in &self.list {
            if !world.visible[d.node] {
                continue;
            }
            let g = world.global[d.node];
            if (g.w_axis.truncate() - center).length() > 4.0 {
                continue;
            }
            let inv = g.inverse();
            let q = inv.transform_point3(center + shift);
            let c = q.clamp(d.bmin, d.bmax);
            let wc = g.transform_point3(c);
            let mut delta = center + shift - wc;
            delta.y = 0.0;
            let dist = delta.length();
            if dist < radius && dist > 1e-5 {
                shift += delta / dist * (radius - dist);
            }
        }
        shift
    }
}
