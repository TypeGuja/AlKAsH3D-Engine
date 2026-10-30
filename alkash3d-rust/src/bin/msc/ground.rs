//! Земля и стены: твёрдые треугольники статики (см. `World::build`),
//! разложенные по сетке 4x4 м в плоскости XZ. Высота под точкой — луч
//! вниз; стены — выталкивание сферы из крутых (|n.y| < 0.6) треугольников
//! по ближайшей точке: игрок скользит вдоль стены, а не застревает.
//! Коллайдеры оригинала (MeshCollider) не используются — ходим по видимой
//! геометрии.

use std::collections::HashMap;

use alkash3d_rs::math::Vec3;

use crate::world::Triangles;

const CELL: f32 = 4.0;

pub struct Ground {
    tris: Vec<[Vec3; 3]>,
    owner: Vec<u32>,
    steep: Vec<bool>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

/// Ближайшая к `p` точка треугольника (Ericson, "Real-Time Collision Detection" 5.1.5).
fn closest_on_triangle(p: Vec3, [a, b, c]: [Vec3; 3]) -> Vec3 {
    let ab = b - a;
    let ac = c - a;
    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        return a + ab * (d1 / (d1 - d3));
    }
    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        return a + ac * (d2 / (d2 - d6));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        return b + (c - b) * ((d4 - d3) / ((d4 - d3) + (d5 - d6)));
    }
    let denom = 1.0 / (va + vb + vc);
    a + ab * (vb * denom) + ac * (vc * denom)
}

/// Луч—треугольник (Мёллер–Трумбор): расстояние вдоль `d` или None.
pub fn ray_triangle(o: Vec3, d: Vec3, [v0, v1, v2]: [Vec3; 3]) -> Option<f32> {
    let e1 = v1 - v0;
    let e2 = v2 - v0;
    let h = d.cross(e2);
    let det = e1.dot(h);
    if det.abs() < 1e-9 {
        return None;
    }
    let f = 1.0 / det;
    let s = o - v0;
    let u = f * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = f * d.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = f * e2.dot(q);
    (t > 0.0).then_some(t)
}

impl Ground {
    pub fn new(tris: Triangles) -> Ground {
        let mut g = Ground { steep: Vec::with_capacity(tris.len()), tris: Vec::with_capacity(tris.len()), owner: Vec::new(), grid: HashMap::new() };
        for (tri, owner) in tris {
            let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
            if n.length_squared() < 1e-10 {
                continue;
            }
            let (x0, x1) = (tri.iter().map(|p| p.x).fold(f32::MAX, f32::min), tri.iter().map(|p| p.x).fold(f32::MIN, f32::max));
            let (z0, z1) = (tri.iter().map(|p| p.z).fold(f32::MAX, f32::min), tri.iter().map(|p| p.z).fold(f32::MIN, f32::max));
            // Плоскости на всю карту (дно, вода-плита) не вносим во все клетки.
            if (x1 - x0) > 400.0 || (z1 - z0) > 400.0 {
                continue;
            }
            let idx = g.tris.len() as u32;
            g.steep.push(n.normalize().y.abs() < 0.6);
            g.tris.push(tri);
            g.owner.push(owner);
            for cx in cell(x0)..=cell(x1) {
                for cz in cell(z0)..=cell(z1) {
                    g.grid.entry((cx, cz)).or_default().push(idx);
                }
            }
        }
        g
    }

    pub fn triangles(&self) -> usize {
        self.tris.len()
    }

    /// Высота самой верхней поверхности под точкой `p` (не выше p.y).
    pub fn height_below(&self, p: Vec3) -> Option<f32> {
        let list = self.grid.get(&(cell(p.x), cell(p.z)))?;
        let mut best: Option<f32> = None;
        for &i in list {
            let [a, b, c] = self.tris[i as usize];
            // Барицентрические координаты в плоскости XZ.
            let d = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
            if d.abs() < 1e-8 {
                continue;
            }
            let l1 = ((b.z - c.z) * (p.x - c.x) + (c.x - b.x) * (p.z - c.z)) / d;
            let l2 = ((c.z - a.z) * (p.x - c.x) + (a.x - c.x) * (p.z - c.z)) / d;
            let l3 = 1.0 - l1 - l2;
            if l1 < 0.0 || l2 < 0.0 || l3 < 0.0 {
                continue;
            }
            let y = l1 * a.y + l2 * b.y + l3 * c.y;
            if y <= p.y && best.is_none_or(|h| y > h) {
                best = Some(y);
            }
        }
        best
    }

    /// Горизонтальный сдвиг, выталкивающий сферу (`center`, `radius`) из
    /// крутых поверхностей. Несколько итераций — для углов между стенами.
    pub fn push_out(&self, center: Vec3, radius: f32) -> Vec3 {
        let mut c = center;
        for _ in 0..3 {
            let mut moved = false;
            let (x0, x1) = (cell(c.x - radius), cell(c.x + radius));
            let (z0, z1) = (cell(c.z - radius), cell(c.z + radius));
            for cx in x0..=x1 {
                for cz in z0..=z1 {
                    let Some(list) = self.grid.get(&(cx, cz)) else { continue };
                    for &i in list {
                        if !self.steep[i as usize] {
                            continue;
                        }
                        let q = closest_on_triangle(c, self.tris[i as usize]);
                        // Касание — по полному расстоянию (крыша над головой не стена),
                        // выталкиваем по горизонтали (по вертикали держит пол).
                        if (c - q).length() >= radius {
                            continue;
                        }
                        let mut d = c - q;
                        d.y = 0.0;
                        let dist = d.length();
                        if dist > 1e-5 {
                            c += d / dist * (radius - dist);
                            moved = true;
                        }
                    }
                }
            }
            if !moved {
                break;
            }
        }
        c - center
    }

    /// Самое глубокое проникновение сферы (`p`, `r`) в любую поверхность:
    /// (нормаль наружу, глубина). Для физических тел.
    pub fn contact(&self, p: Vec3, r: f32) -> Option<(Vec3, f32)> {
        let mut best: Option<(Vec3, f32)> = None;
        for cx in cell(p.x - r)..=cell(p.x + r) {
            for cz in cell(p.z - r)..=cell(p.z + r) {
                let Some(list) = self.grid.get(&(cx, cz)) else { continue };
                for &i in list {
                    let tri = self.tris[i as usize];
                    let q = closest_on_triangle(p, tri);
                    let d = p - q;
                    let dist = d.length();
                    if dist >= r {
                        continue;
                    }
                    let face = (tri[1] - tri[0]).cross(tri[2] - tri[0]).normalize_or_zero();
                    // Ровно на плоскости — по нормали грани, в сторону точки.
                    let n = if dist > 1e-5 { d / dist } else if face.dot(d) >= 0.0 { face } else { -face };
                    let depth = r - dist;
                    if best.is_none_or(|b| depth > b.1) {
                        best = Some((n, depth));
                    }
                }
            }
        }
        best
    }

    /// Расстояние до первой поверхности по лучу (`d` — единичный), не дальше `max`.
    pub fn raycast(&self, o: Vec3, d: Vec3, max: f32) -> Option<f32> {
        let mut best: Option<f32> = None;
        let steps = (max / CELL * 2.0).ceil() as i32 + 1;
        let mut seen = std::collections::HashSet::new();
        for k in 0..=steps {
            let p = o + d * (max * k as f32 / steps as f32);
            for cx in cell(p.x) - 1..=cell(p.x) + 1 {
                for cz in cell(p.z) - 1..=cell(p.z) + 1 {
                    if !seen.insert((cx, cz)) {
                        continue;
                    }
                    let Some(list) = self.grid.get(&(cx, cz)) else { continue };
                    for &i in list {
                        if let Some(t) = ray_triangle(o, d, self.tris[i as usize]) {
                            if t < max && best.is_none_or(|b| t < b) {
                                best = Some(t);
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// Узел мира, чья крутая поверхность ближе `radius` к `center` (отладка стен).
    pub fn blocker(&self, center: Vec3, radius: f32) -> Option<u32> {
        let list = self.grid.get(&(cell(center.x), cell(center.z)))?;
        list.iter().copied().find(|&i| {
            let q = closest_on_triangle(center, self.tris[i as usize]);
            self.steep[i as usize] && (center - q).length() < radius
        }).map(|i| self.owner[i as usize])
    }

    /// Есть ли крутая поверхность на отрезке a→b (машина врезалась).
    pub fn wall_between(&self, a: Vec3, b: Vec3) -> bool {
        let dir = b - a;
        let len = dir.length();
        if len < 1e-5 {
            return false;
        }
        let d = dir / len;
        let mut cells = vec![(cell(a.x), cell(a.z))];
        if (cell(b.x), cell(b.z)) != cells[0] {
            cells.push((cell(b.x), cell(b.z)));
        }
        for c in cells {
            let Some(list) = self.grid.get(&c) else { continue };
            for &i in list {
                if !self.steep[i as usize] {
                    continue;
                }
                let [v0, v1, v2] = self.tris[i as usize];
                // Мёллер–Трумбор.
                let e1 = v1 - v0;
                let e2 = v2 - v0;
                let h = d.cross(e2);
                let det = e1.dot(h);
                if det.abs() < 1e-7 {
                    continue;
                }
                let f = 1.0 / det;
                let s = a - v0;
                let u = f * s.dot(h);
                if !(0.0..=1.0).contains(&u) {
                    continue;
                }
                let q = s.cross(e1);
                let v = f * d.dot(q);
                if v < 0.0 || u + v > 1.0 {
                    continue;
                }
                let t = f * e2.dot(q);
                if t > 0.0 && t < len {
                    return true;
                }
            }
        }
        false
    }
}
