//! Столкновения игрока с геометрией сцены: треугольники статических мешей
//! в мировых координатах, разложенные по сетке XZ. Игрок — несколько сфер
//! (низ/середина/голова), каждая выталкивается из ближайших треугольников.

use std::collections::HashMap;

use alkash3d_rs::math::Vec3;

const CELL: f32 = 8.0;

#[derive(Clone, Copy)]
pub struct Tri {
    pub a: Vec3,
    pub b: Vec3,
    pub c: Vec3,
    pub n: Vec3,
    /// Габариты — быстро отбросить далёкие треугольники до точной проверки.
    lo: Vec3,
    hi: Vec3,
}

impl Tri {
    pub fn new(a: Vec3, b: Vec3, c: Vec3) -> Option<Tri> {
        let n = (b - a).cross(c - a);
        let len = n.length();
        (len > 1e-6).then(|| Tri { a, b, c, n: n / len, lo: a.min(b).min(c), hi: a.max(b).max(c) })
    }

    /// Ближайшая к `p` точка треугольника (Ericson, "Real-Time Collision Detection").
    pub fn closest(&self, p: Vec3) -> Vec3 {
        let (a, b, c) = (self.a, self.b, self.c);
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

    /// Луч (Möller–Trumbore): расстояние до пересечения.
    pub fn ray(&self, o: Vec3, d: Vec3) -> Option<f32> {
        let e1 = self.b - self.a;
        let e2 = self.c - self.a;
        let p = d.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-8 {
            return None;
        }
        let inv = 1.0 / det;
        let s = o - self.a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = d.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = e2.dot(q) * inv;
        (t > 0.0).then_some(t)
    }

    fn min_max(&self) -> (Vec3, Vec3) {
        (self.lo, self.hi)
    }

    /// Пересекаются ли габариты треугольника с кубом [lo, hi].
    fn overlaps(&self, lo: Vec3, hi: Vec3) -> bool {
        self.lo.x <= hi.x && self.hi.x >= lo.x && self.lo.y <= hi.y && self.hi.y >= lo.y && self.lo.z <= hi.z && self.hi.z >= lo.z
    }
}

pub struct Collider {
    tris: Vec<Tri>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

fn cell(v: f32) -> i32 {
    (v / CELL).floor() as i32
}

impl Collider {
    pub fn new(tris: Vec<Tri>) -> Collider {
        let mut grid: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
        for (i, t) in tris.iter().enumerate() {
            let (lo, hi) = t.min_max();
            // Гигантские треугольники (коробки-"затемнения" вокруг уровня) тоже
            // кладём во все клетки, но ограничиваем размах разумным пределом.
            let (x0, x1) = (cell(lo.x).max(-200), cell(hi.x).min(200));
            let (z0, z1) = (cell(lo.z).max(-200), cell(hi.z).min(200));
            for x in x0..=x1 {
                for z in z0..=z1 {
                    grid.entry((x, z)).or_default().push(i as u32);
                }
            }
        }
        Collider { tris, grid }
    }

    pub fn len(&self) -> usize {
        self.tris.len()
    }

    /// Треугольники клеток вокруг [lo, hi], чьи габариты с ним пересекаются.
    /// Без сбора и сортировки id: у детальных объектов (счётчик жизней,
    /// дефибриллятор) в клетках десятки тысяч треугольников, а повтор
    /// треугольника из соседней клетки безвреден — из него уже вытолкнули.
    fn near<'a>(&'a self, lo: Vec3, hi: Vec3) -> impl Iterator<Item = &'a Tri> + 'a {
        let (x0, x1, z0, z1) = (cell(lo.x), cell(hi.x), cell(lo.z), cell(hi.z));
        (x0..=x1)
            .flat_map(move |x| (z0..=z1).map(move |z| (x, z)))
            .filter_map(move |k| self.grid.get(&k))
            .flatten()
            .map(move |&i| &self.tris[i as usize])
            .filter(move |t| t.overlaps(lo, hi))
    }

    /// Вытолкнуть сферу из геометрии (плюс `extra` — двигающиеся треугольники,
    /// например двери). Возвращает сдвиг и нормаль самой "напольной" опоры.
    pub fn push_sphere(&self, center: Vec3, r: f32, extra: &[Tri]) -> (Vec3, Option<Vec3>) {
        let mut c = center;
        let mut ground: Option<Vec3> = None;
        for _ in 0..4 {
            let mut moved = false;
            let (lo, hi) = (c - Vec3::splat(r), c + Vec3::splat(r));
            for t in self.near(lo, hi).chain(extra.iter()) {
                let p = t.closest(c);
                let d = c - p;
                let dist2 = d.length_squared();
                if dist2 < r * r {
                    let dist = dist2.sqrt();
                    // Центр ровно на плоскости — толкаем по нормали треугольника.
                    let n = if dist > 1e-5 { d / dist } else { t.n };
                    c += n * (r - dist + 1e-3);
                    moved = true;
                    if n.y > 0.6 && ground.is_none_or(|g| n.y > g.y) {
                        ground = Some(n);
                    }
                }
            }
            if !moved {
                break;
            }
        }
        (c - center, ground)
    }

    /// Ближайшее пересечение луча (o, d — единичный) не дальше max: расстояние
    /// и нормаль поверхности, обращённая навстречу лучу.
    pub fn ray_hit(&self, o: Vec3, d: Vec3, max: f32) -> Option<(f32, Vec3)> {
        let mut best: Option<(f32, Vec3)> = None;
        let steps = (max / (CELL * 0.5)).ceil() as i32 + 1;
        let mut seen = std::collections::HashSet::new();
        for s in 0..=steps {
            let p = o + d * (s as f32 * CELL * 0.5).min(max);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let Some(v) = self.grid.get(&(cell(p.x) + dx, cell(p.z) + dz)) else { continue };
                    for &i in v {
                        if !seen.insert(i) {
                            continue;
                        }
                        let t = &self.tris[i as usize];
                        if let Some(h) = t.ray(o, d) {
                            if h <= max && best.is_none_or(|b| h < b.0) {
                                best = Some((h, if t.n.dot(d) > 0.0 { -t.n } else { t.n }));
                            }
                        }
                    }
                }
            }
        }
        best
    }

    /// Ближайшее пересечение луча (o, d — единичный) не дальше max.
    pub fn ray(&self, o: Vec3, d: Vec3, max: f32, extra: &[Tri]) -> Option<f32> {
        let mut best: Option<f32> = None;
        let mut take = |t: f32| {
            if t <= max && best.is_none_or(|b| t < b) {
                best = Some(t);
            }
        };
        for t in extra {
            if let Some(h) = t.ray(o, d) {
                take(h);
            }
        }
        // Шагаем по клеткам вдоль луча (грубо, но лучи у нас короткие).
        let steps = (max / (CELL * 0.5)).ceil() as i32 + 1;
        let mut seen = std::collections::HashSet::new();
        for s in 0..=steps {
            let p = o + d * (s as f32 * CELL * 0.5);
            for dx in -1..=1 {
                for dz in -1..=1 {
                    if let Some(v) = self.grid.get(&(cell(p.x) + dx, cell(p.z) + dz)) {
                        for &i in v {
                            if seen.insert(i) {
                                if let Some(h) = self.tris[i as usize].ray(o, d) {
                                    take(h);
                                }
                            }
                        }
                    }
                }
            }
        }
        best
    }
}
