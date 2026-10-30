//! Геометрия подъезда пятиэтажки: лестничная клетка 2.5 x 5.4 м, этаж
//! 2.8 м, два марша по 10 подступенков (0.14 x 0.30) на этаж, между ними
//! промежуточная площадка с окном во двор. Всё в метрах, X — поперёк
//! клетки, Y — вверх, Z — вглубь (от этажной площадки к окну).
//!
//! Марш A идёт с этажной площадки n вверх к промежуточной (x 1.3..2.5),
//! марш B — с промежуточной на площадку n+1 (x 0..1.2); между ними
//! просвет 0.1 м с перилами. Под первым маршем B — кладовка.

use alkash3d_rs::engine::Vertex;
use alkash3d_rs::math::Vec3;

pub const H: f32 = 2.8;
pub const HALF: f32 = 1.4;
pub const W: f32 = 2.5;
pub const D: f32 = 5.4;
pub const LAND_Z: f32 = 1.5;
pub const MID_Z: f32 = 4.2;
pub const SLAB: f32 = 0.2;
pub const FLOORS: usize = 5;
pub const ROOF: f32 = FLOORS as f32 * H - SLAB;
pub const A_X: f32 = 1.3;
pub const B_X: f32 = 1.2;
const RISERS: usize = 10;
const RISE: f32 = HALF / RISERS as f32;
const RUN: f32 = (MID_Z - LAND_Z) / (RISERS - 1) as f32;
const PAINT_H: f32 = 1.5;
const WALL_T: f32 = 0.38;
/// Окно промежуточной площадки (по X) и его низ/верх над её полом.
const WIN_X: [f32; 2] = [0.55, 1.95];
const WIN_Y: [f32; 2] = [0.85, 2.3];
/// Двор: снег и дом напротив.
pub const GROUND_Y: f32 = -0.4;
const OPPOSITE_Z: f32 = D + 26.0;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Mat {
    Paint,
    White,
    Tread,
    Concrete,
    Tiles,
    Metal,
    Rail,
    Door,
    Wood,
    Glass,
    Snow,
    Facade,
    /// ДОБАВЛЕНО (текстуры двора): штукатурка нашего дома, цоколь,
    /// кирпич ограды, профлист, крашеная сталь с ржавчиной.
    Plaster,
    Plinth,
    Brick,
    Sheet,
    Steel,
    Black,
    Brass,
    WinDark,
    WinLit,
    Rug,
    Paper,
    Soot,
    Scribble,
    Web,
    Digits,
    Warning,
    Wet,
    Frost,
    Crack,
}

pub const MATS: [Mat; 31] = [
    Mat::Paint,
    Mat::White,
    Mat::Tread,
    Mat::Concrete,
    Mat::Tiles,
    Mat::Metal,
    Mat::Rail,
    Mat::Door,
    Mat::Wood,
    Mat::Glass,
    Mat::Snow,
    Mat::Facade,
    Mat::Plaster,
    Mat::Plinth,
    Mat::Brick,
    Mat::Sheet,
    Mat::Steel,
    Mat::Black,
    Mat::Brass,
    Mat::WinDark,
    Mat::WinLit,
    Mat::Rug,
    Mat::Paper,
    Mat::Soot,
    Mat::Scribble,
    Mat::Web,
    Mat::Digits,
    Mat::Warning,
    Mat::Wet,
    Mat::Frost,
    Mat::Crack,
];

#[derive(Default)]
pub struct Geo {
    pub v: Vec<Vertex>,
    pub i: Vec<u32>,
}

/// ДОБАВЛЕНО (forward-декали): декаль на плоскости — ячейка атласа
/// `tex::decal_atlas` (`sub` — часть ячейки, 0..1), центр, нормаль плоскости,
/// верх картинки, размер, цвет и непрозрачность, шероховатость (<0 — не менять).
pub struct DecalSpot {
    pub cell: usize,
    pub sub: [f32; 4],
    pub center: Vec3,
    pub normal: Vec3,
    pub up: Vec3,
    pub w: f32,
    pub h: f32,
    pub color: [f32; 4],
    pub roughness: f32,
}

/// Что лампа делает.
#[derive(Clone, Copy, PartialEq)]
pub enum LampKind {
    Steady,
    /// Плохой контакт в патроне — гаснет и вспыхивает.
    Flicker,
    /// Перегорела: колба есть, света нет.
    Dead,
}

pub struct Lamp {
    pub pos: Vec3,
    /// Сила света, кд.
    pub candela: f32,
    pub kelvin: f32,
    pub kind: LampKind,
    /// Радиус колбы (видимая светящаяся сфера), м.
    pub bulb_r: f32,
    /// Светильник с отражателем, светит вниз: (половина угла конуса,
    /// начало спада), рад. None — голая лампа во все стороны.
    pub cone: Option<(f32, f32)>,
    /// Прозрачная колба: видна нить, тени резкие (источник — нить ~1 см).
    pub clear: bool,
    /// ДОБАВЛЕНО (двор перед домом): дальность, м (0 — по силе света, см.
    /// main.rs). Лампы у чужих подъездов — 10 м: теневых карт у точечного
    /// света 8 на кадр, лампа без своей светила бы сквозь стены подъезда.
    pub range: f32,
}

pub struct Building {
    pub geo: Vec<Geo>,
    pub lamps: Vec<Lamp>,
    /// Индексы ламп-«окон» напротив (светят наружу, не вниз).
    pub window_lamps: Vec<usize>,
    /// ДОБАВЛЕНО (выход на улицу): полотно входной двери — отдельно от стен,
    /// чтобы поворачиваться на петле; координаты — от петли (DOOR_HINGE).
    pub door: Vec<Geo>,
    /// ДОБАВЛЕНО (группы видимости): двор за домом (виден из окон клетки) и
    /// двор перед домом (виден только в открытую входную дверь). `geo` —
    /// сама клетка. Раньше всё было в одних мешах по материалу, и стоя в
    /// подъезде видеокарта каждый кадр обрабатывала вершины всего двора.
    pub back: Vec<Geo>,
    pub front: Vec<Geo>,
    /// ДОБАВЛЕНО (forward-декали): пятна на поверхностях без своей геометрии.
    pub decals: Vec<DecalSpot>,
}

fn v3(x: f32, y: f32, z: f32) -> Vec3 {
    Vec3::new(x, y, z)
}

impl Building {
    fn empty() -> Building {
        Building { geo: Self::no_geo(), lamps: Vec::new(), window_lamps: Vec::new(), door: Vec::new(), back: Vec::new(), front: Vec::new(), decals: Vec::new() }
    }

    fn no_geo() -> Vec<Geo> {
        MATS.iter().map(|_| Geo::default()).collect()
    }

    fn g(&mut self, m: Mat) -> &mut Geo {
        &mut self.geo[MATS.iter().position(|&x| x == m).unwrap()]
    }

    /// Четырёхугольник p0..p3 (по кругу) с UV в углах и нормалью `n`.
    /// Касательная — направление роста u, знак битангенса — по росту v.
    fn quad4(&mut self, m: Mat, p: [Vec3; 4], uv: [[f32; 2]; 4], n: Vec3, col: [f32; 3]) {
        let (e1, e2) = (p[1] - p[0], p[3] - p[0]);
        let (d1, d2) = ([uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]], [uv[3][0] - uv[0][0], uv[3][1] - uv[0][1]]);
        let det = d1[0] * d2[1] - d2[0] * d1[1];
        let r = if det.abs() > 1e-12 { 1.0 / det } else { 1.0 };
        let t = (e1 * d2[1] - e2 * d1[1]) * r;
        let b = (e2 * d1[0] - e1 * d2[0]) * r;
        let n = n.normalize();
        let t = (t - n * n.dot(t)).normalize_or_zero();
        let t = if t == Vec3::ZERO { n.any_orthonormal_vector() } else { t };
        let w = if n.cross(t).dot(b) < 0.0 { -1.0 } else { 1.0 };
        let g = self.g(m);
        let base = g.v.len() as u32;
        for k in 0..4 {
            g.v.push(Vertex::with_normal_uv_tangent(p[k].x, p[k].y, p[k].z, n.x, n.y, n.z, col[0], col[1], col[2], 1.0, uv[k][0], uv[k][1], [t.x, t.y, t.z, w]));
        }
        g.i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Плоский прямоугольник с UV из мировых координат (тайл `s` метров):
    /// проекция по главной оси нормали, v растёт вниз по стене.
    fn quad_w(&mut self, m: Mat, p: [Vec3; 4], n: Vec3, s: f32, col: [f32; 3]) {
        self.quad_ws(m, p, n, [s, s], [0.0, 0.0], col);
    }

    /// То же с разным размером тайла по u и v и сдвигом начала (`o` — в
    /// метрах по u/v), чтобы швы текстуры легли на нужные места.
    fn quad_ws(&mut self, m: Mat, p: [Vec3; 4], n: Vec3, s: [f32; 2], o: [f32; 2], col: [f32; 3]) {
        let a = n.abs();
        let uv = p.map(|q| {
            let (u, v) = if a.y >= a.x && a.y >= a.z {
                (q.x, q.z)
            } else if a.x >= a.z {
                (q.z, -q.y)
            } else {
                (q.x, -q.y)
            };
            [(u - o[0]) / s[0], (v - o[1]) / s[1]]
        });
        self.quad4(m, p, uv, n, col);
    }

    /// Ящик по осям с фаской на рёбрах (у настоящих вещей кромка не бывает
    /// идеально острой — на фаске ловится блик): грани, 12 скосов, 8 углов.
    fn cube(&mut self, m: Mat, lo: Vec3, hi: Vec3, s: f32, col: [f32; 3]) {
        let ext = hi - lo;
        let c = (ext.min_element() * 0.2).min(0.004);
        if c < 0.0006 {
            self.cube_sharp(m, lo, hi, s, col);
            return;
        }
        let (a, b) = (lo + Vec3::splat(c), hi - Vec3::splat(c));
        let e = [Vec3::X, Vec3::Y, Vec3::Z];
        let outer = |sg: f32, k: usize| if sg > 0.0 { hi[k] } else { lo[k] };
        let inner = |sg: f32, k: usize| if sg > 0.0 { b[k] } else { a[k] };
        let mk = |vals: [(usize, f32); 3]| {
            let mut p = Vec3::ZERO;
            for (k, v) in vals {
                p[k] = v;
            }
            p
        };
        // грани
        for i in 0..3 {
            let (u, v) = ((i + 1) % 3, (i + 2) % 3);
            for sg in [-1.0f32, 1.0] {
                let w = outer(sg, i);
                let p = [mk([(i, w), (u, a[u]), (v, a[v])]), mk([(i, w), (u, b[u]), (v, a[v])]), mk([(i, w), (u, b[u]), (v, b[v])]), mk([(i, w), (u, a[u]), (v, b[v])])];
                self.quad_w(m, p, e[i] * sg, s, col);
            }
        }
        // скосы рёбер
        for i in 0..3 {
            for j in (i + 1)..3 {
                let k = 3 - i - j;
                for si in [-1.0f32, 1.0] {
                    for sj in [-1.0f32, 1.0] {
                        let p1 = |t: f32| mk([(i, outer(si, i)), (j, inner(sj, j)), (k, t)]);
                        let p2 = |t: f32| mk([(i, inner(si, i)), (j, outer(sj, j)), (k, t)]);
                        let n = (e[i] * si + e[j] * sj).normalize();
                        self.quad_w(m, [p1(a[k]), p1(b[k]), p2(b[k]), p2(a[k])], n, s, col);
                    }
                }
            }
        }
        // углы
        for sx in [-1.0f32, 1.0] {
            for sy in [-1.0f32, 1.0] {
                for sz in [-1.0f32, 1.0] {
                    let sg = [sx, sy, sz];
                    let q = |o: usize| mk([(0, if o == 0 { outer(sg[0], 0) } else { inner(sg[0], 0) }), (1, if o == 1 { outer(sg[1], 1) } else { inner(sg[1], 1) }), (2, if o == 2 { outer(sg[2], 2) } else { inner(sg[2], 2) })]);
                    let n = Vec3::new(sx, sy, sz).normalize();
                    self.quad_w(m, [q(0), q(1), q(2), q(2)], n, s, col);
                }
            }
        }
    }

    /// Ящик без фаски (совсем мелкие детали).
    fn cube_sharp(&mut self, m: Mat, lo: Vec3, hi: Vec3, s: f32, col: [f32; 3]) {
        let (a, b) = (lo, hi);
        let c = |x: f32, y: f32, z: f32| v3(x, y, z);
        self.quad_w(m, [c(a.x, a.y, b.z), c(b.x, a.y, b.z), c(b.x, b.y, b.z), c(a.x, b.y, b.z)], Vec3::Z, s, col);
        self.quad_w(m, [c(b.x, a.y, a.z), c(a.x, a.y, a.z), c(a.x, b.y, a.z), c(b.x, b.y, a.z)], -Vec3::Z, s, col);
        self.quad_w(m, [c(b.x, a.y, b.z), c(b.x, a.y, a.z), c(b.x, b.y, a.z), c(b.x, b.y, b.z)], Vec3::X, s, col);
        self.quad_w(m, [c(a.x, a.y, a.z), c(a.x, a.y, b.z), c(a.x, b.y, b.z), c(a.x, b.y, a.z)], -Vec3::X, s, col);
        self.quad_w(m, [c(a.x, b.y, b.z), c(b.x, b.y, b.z), c(b.x, b.y, a.z), c(a.x, b.y, a.z)], Vec3::Y, s, col);
        self.quad_w(m, [c(a.x, a.y, a.z), c(b.x, a.y, a.z), c(b.x, a.y, b.z), c(a.x, a.y, b.z)], -Vec3::Y, s, col);
    }

    /// Вертикальный цилиндр (трубы, банка): боковая поверхность и верх.
    fn cyl(&mut self, m: Mat, base: Vec3, h: f32, r: f32, segs: u32, col: [f32; 3]) {
        let tau = std::f32::consts::TAU;
        for k in 0..segs {
            let (a0, a1) = (tau * k as f32 / segs as f32, tau * (k + 1) as f32 / segs as f32);
            let (d0, d1) = (v3(a0.cos(), 0.0, a0.sin()), v3(a1.cos(), 0.0, a1.sin()));
            let (p0, p1) = (base + d0 * r, base + d1 * r);
            let up = Vec3::Y * h;
            let (u0, u1) = (r * a0 / 0.5, r * a1 / 0.5);
            self.quad4(m, [p0, p1, p1 + up, p0 + up], [[u0, h / 0.5], [u1, h / 0.5], [u1, 0.0], [u0, 0.0]], (d0 + d1).normalize(), col);
            let c = base + up;
            self.quad4(m, [c, c + d0 * r, c + d1 * r, c], [[0.5, 0.5], [0.5 + d0.x * 0.5, 0.5 + d0.z * 0.5], [0.5 + d1.x * 0.5, 0.5 + d1.z * 0.5], [0.5, 0.5]], Vec3::Y, col);
        }
    }

    /// Эллипсоид (сугробы): нижняя половина срезана землёй, если `half`.
    fn ellipsoid(&mut self, m: Mat, c: Vec3, r: Vec3, half: bool, col: [f32; 3]) {
        let (rings, segs) = (8u32, 16u32);
        let top = if half { rings / 2 } else { rings };
        // бугры: радиус чуть гуляет по направлению (у каждого — свой узор)
        let seed = (c.x * 7.3 + c.z * 3.1).fract() * 6.28;
        // множитель sin²θ: у макушки (полюса) бугров нет — иначе точки
        // полюса расходятся по высоте и сходятся «звездой»
        let bump = |th: f32, ph: f32| 1.0 + (0.08 * (ph * 3.0 + seed).sin() + 0.05 * (ph * 5.0 + th * 2.0 + seed * 2.0).sin()) * th.sin() * th.sin();
        let pt = |i: u32, j: u32| {
            let th = std::f32::consts::PI * i as f32 / rings as f32;
            let ph = std::f32::consts::TAU * (j % segs) as f32 / segs as f32;
            let d = v3(th.sin() * ph.cos(), th.cos(), th.sin() * ph.sin());
            let k = bump(th, ph);
            // нормаль — по соседним точкам поверхности
            let e = 0.02;
            let at = |t: f32, p: f32| v3(t.sin() * p.cos(), t.cos(), t.sin() * p.sin()) * r * bump(t, p);
            let n = (at(th + e, ph) - at(th - e, ph)).cross(at(th, ph + e) - at(th, ph - e)).normalize_or(d);
            let n = if n.dot(d) < 0.0 { -n } else { n };
            (c + d * r * k, n)
        };
        let g = self.g(m);
        for i in 0..top {
            for j in 0..segs {
                let corners = [pt(i, j), pt(i, j + 1), pt(i + 1, j + 1), pt(i + 1, j)];
                let base = g.v.len() as u32;
                for (p, n) in corners {
                    let t = n.cross(Vec3::Y).normalize_or(Vec3::X);
                    g.v.push(Vertex::with_normal_uv_tangent(p.x, p.y, p.z, n.x, n.y, n.z, col[0], col[1], col[2], 1.0, p.x / 4.0, p.z / 4.0, [t.x, t.y, t.z, 1.0]));
                }
                g.i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    /// Сугроб-холм: высота над землёй (1 - s²)² по радиусу s, у края наклон
    /// нулевой — холм уходит в снег на земле без борта (у полуэллипсоида
    /// край отвесный, и сугроб выглядит подушкой), соседние сливаются в вал.
    /// Край на 1.5 см ниже земли, чтобы не было щели.
    fn mound(&mut self, m: Mat, c: Vec3, r: Vec3, col: [f32; 3]) {
        let (rings, segs) = (10u32, 24u32);
        let seed = (c.x * 7.3 + c.z * 3.1).fract() * 6.28;
        // контур в плане неровный, по склону — пологие волны надува
        let at = |s: f32, ph: f32| {
            let e = 1.0 + 0.12 * (ph * 2.0 + seed).sin() + 0.07 * (ph * 5.0 + seed * 3.0).sin();
            let q = 1.0 - s * s;
            let h = q * q * (1.0 + 0.12 * s * (ph * 3.0 + seed * 2.0).cos()) * r.y - 0.015;
            c + v3(ph.cos() * s * e * r.x, h, ph.sin() * s * e * r.z)
        };
        let pt = |i: u32, j: u32| {
            let s = i as f32 / rings as f32;
            let ph = std::f32::consts::TAU * (j % segs) as f32 / segs as f32;
            let e = 0.01;
            let n = (at(s + e, ph) - at((s - e).max(0.0), ph)).cross(at(s, ph - e) - at(s, ph + e));
            let n = if i == 0 { Vec3::Y } else { n.normalize_or(Vec3::Y) };
            let n = if n.y < 0.0 { -n } else { n };
            (at(s, ph), n)
        };
        let g = self.g(m);
        for i in 0..rings {
            for j in 0..segs {
                // обход — как у `ellipsoid` (i растёт от макушки)
                let corners = [pt(i, j), pt(i, j + 1), pt(i + 1, j + 1), pt(i + 1, j)];
                let base = g.v.len() as u32;
                for (p, n) in corners {
                    let t = n.cross(Vec3::Y).normalize_or(Vec3::X);
                    g.v.push(Vertex::with_normal_uv_tangent(p.x, p.y, p.z, n.x, n.y, n.z, col[0], col[1], col[2], 1.0, p.x / 4.0, p.z / 4.0, [t.x, t.y, t.z, 1.0]));
                }
                g.i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }

    /// Труба вдоль p0→p1 (поручни, полозья): `sides` граней с плавными
    /// нормалями по окружности; `flat` < 1 — сплющенная по вертикали
    /// (профиль поручня-«грибка»).
    fn tube(&mut self, m: Mat, p0: Vec3, p1: Vec3, r: f32, flat: f32, sides: u32, col: [f32; 3]) {
        let d = (p1 - p0).normalize();
        let mut side = d.cross(Vec3::Y);
        if side.length() < 1e-3 {
            side = Vec3::X;
        }
        let side = side.normalize();
        let up = side.cross(d).normalize();
        let up = if up.y < 0.0 { -up } else { up };
        let len = (p1 - p0).length();
        let ring = |k: u32| {
            let a = std::f32::consts::TAU * k as f32 / sides as f32;
            let off = side * (a.cos() * r) + up * (a.sin() * r * flat);
            let n = (side * a.cos() + up * (a.sin() / flat.max(0.1))).normalize();
            (off, n)
        };
        let g = self.g(m);
        for k in 0..sides {
            let ((o0, n0), (o1, n1)) = (ring(k), ring(k + 1));
            let base = g.v.len() as u32;
            let t = [d.x, d.y, d.z, 1.0];
            let u0 = k as f32 / sides as f32;
            let u1 = (k + 1) as f32 / sides as f32;
            for (p, n, u, v) in [(p0 + o0, n0, u0, 0.0), (p1 + o0, n0, u0, len / 0.5), (p1 + o1, n1, u1, len / 0.5), (p0 + o1, n1, u1, 0.0)] {
                g.v.push(Vertex::with_normal_uv_tangent(p.x, p.y, p.z, n.x, n.y, n.z, col[0], col[1], col[2], 1.0, u, v, t));
            }
            g.i.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        // заглушки на концах (иначе в торце видна изнанка трубы)
        for (c, n) in [(p0, -d), (p1, d)] {
            let g = self.g(m);
            let center = g.v.len() as u32;
            let t = [side.x, side.y, side.z, 1.0];
            g.v.push(Vertex::with_normal_uv_tangent(c.x, c.y, c.z, n.x, n.y, n.z, col[0], col[1], col[2], 1.0, 0.5, 0.5, t));
            for k in 0..=sides {
                let (o, _) = ring(k);
                let p = c + o;
                g.v.push(Vertex::with_normal_uv_tangent(p.x, p.y, p.z, n.x, n.y, n.z, col[0], col[1], col[2], 1.0, 0.5, 0.5, t));
            }
            for k in 0..sides {
                g.i.extend([center, center + 1 + k, center + 2 + k]);
            }
        }
    }

    /// Брус вдоль отрезка p0→p1 (наклонные поручни, полосы перил):
    /// `side` — ось ширины (перпендикулярна отрезку), w x h — сечение.
    fn beam(&mut self, m: Mat, p0: Vec3, p1: Vec3, side: Vec3, w: f32, h: f32, col: [f32; 3]) {
        let d = (p1 - p0).normalize();
        let s = side.normalize() * (w * 0.5);
        let mut up = side.normalize().cross(d);
        if up.y < 0.0 {
            up = -up;
        }
        let u = up * (h * 0.5);
        let len = (p1 - p0).length();
        let face = |me: &mut Self, o: Vec3, a: Vec3, b: Vec3, n: Vec3, la: f32, lb: f32| {
            me.quad4(m, [o, o + a, o + a + b, o + b], [[0.0, 0.0], [la, 0.0], [la, lb], [0.0, lb]], n, col);
        };
        let ax = p1 - p0;
        face(self, p0 - s + u, ax, s * 2.0, up, len, w);
        face(self, p0 - s - u, ax, s * 2.0, -up, len, w);
        face(self, p0 + s - u, ax, u * 2.0, side.normalize(), len, h);
        face(self, p0 - s - u, ax, u * 2.0, -side.normalize(), len, h);
        face(self, p0 - s - u, s * 2.0, u * 2.0, -d, w, h);
        face(self, p1 - s - u, s * 2.0, u * 2.0, d, w, h);
    }

    /// Кусок стены между s0 и s1 вдоль неё: пол там f0..f1, по высоте от
    /// f+lo до f+hi (или до `top` абсолютно). Покраска — низ до 1.5 м над
    /// полом (полоса идёт параллельно маршу), выше — побелка.
    #[allow(clippy::too_many_arguments)]
    fn wall(&mut self, wall: Wall, s0: f32, s1: f32, f0: f32, f1: f32, lo: f32, hi: f32, top: Option<f32>) {
        // Штукатурка не бывает ровной: стена — сетка ~20 см, прогиб вглубь
        // до 3 мм плавным шумом (волны 0.35–0.9 м), нормали — по самой
        // поверхности: под скользящим светом лампы плоскость идёт пятнами
        // блика. Только вглубь — иначе стена закроет декали и листки на ней.
        // На краях прогиб 0: стыки с соседними стенами, полом, потолком и
        // окнами сомкнуты; шум — по координатам стены, поэтому граница
        // покраски общая у обоих кусков.
        let n = wall.normal();
        let seed = wall as i32 * 101 + 7;
        let floor = |s: f32| if s1 > s0 { f0 + (f1 - f0) * (s - s0) / (s1 - s0) } else { f0 };
        let wall_top = |f: f32| top.unwrap_or(f + hi);
        let pos = |s: f32, y: f32| {
            let f = floor(s);
            let edge = (s - s0).min(s1 - s).min(y - (f + lo)).min(wall_top(f) - y).max(0.0);
            let k = smooth01(edge / 0.3);
            let w = vnoise(s / 0.9, y / 0.9, seed) * 0.65 + vnoise(s / 0.35, y / 0.35, seed + 1) * 0.35;
            wall.at(s, y) - n * (0.003 * k * w)
        };
        // касательная — по росту u (вдоль s), v растёт вниз (как у quad4)
        let t0 = (wall.at(s0 + 1.0, 0.0) - wall.at(s0, 0.0)).normalize();
        let tw = if n.cross(t0).dot(-Vec3::Y) < 0.0 { -1.0 } else { 1.0 };
        // делим по границе покраски
        let pieces: [(Mat, f32, f32); 2] = [(Mat::Paint, lo, PAINT_H.min(hi)), (Mat::White, PAINT_H.max(lo), hi)];
        for (m, a, b) in pieces {
            if b <= a {
                continue;
            }
            let last = m == Mat::White && b == hi;
            let span = |s: f32| {
                let f = floor(s);
                (f + a, if last { wall_top(f) } else { f + b })
            };
            let cols = ((s1 - s0) / 0.2).ceil().max(1.0) as usize;
            let rows = {
                let ((a0, b0), (a1, b1)) = (span(s0), span(s1));
                ((b0 - a0).max(b1 - a1) / 0.2).ceil().max(1.0) as usize
            };
            let g = self.g(m);
            let base = g.v.len() as u32;
            for i in 0..=cols {
                let s = s0 + (s1 - s0) * i as f32 / cols as f32;
                let (ya, yb) = span(s);
                let f = floor(s);
                for j in 0..=rows {
                    let y = ya + (yb - ya) * j as f32 / rows as f32;
                    let p = pos(s, y);
                    let e = 0.01;
                    let vn = (pos(s + e, y) - pos(s - e, y)).cross(pos(s, y + e) - pos(s, y - e)).normalize_or(n);
                    let vn = if vn.dot(n) < 0.0 { -vn } else { vn };
                    let t = (t0 - vn * vn.dot(t0)).normalize_or(t0);
                    let off = y - f;
                    let v = if m == Mat::Paint { (PAINT_H - off) / PAINT_H } else { (PAINT_H - off) / 2.0 };
                    g.v.push(Vertex::with_normal_uv_tangent(p.x, p.y, p.z, vn.x, vn.y, vn.z, 1.0, 1.0, 1.0, 1.0, s / 2.0, v, [t.x, t.y, t.z, tw]));
                }
            }
            let at = |i: usize, j: usize| base + (i * (rows + 1) + j) as u32;
            for i in 0..cols {
                for j in 0..rows {
                    let q = [at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
                    g.i.extend([q[0], q[1], q[2], q[0], q[2], q[3]]);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Wall {
    /// x = 0, смотрит в +X, s = z
    Left,
    /// x = W, смотрит в −X, s = z
    Right,
    /// z = 0, смотрит в +Z, s = x
    Front,
    /// z = D, смотрит в −Z, s = x
    Back,
}

impl Wall {
    fn at(self, s: f32, y: f32) -> Vec3 {
        match self {
            Wall::Left => v3(0.0, y, s),
            Wall::Right => v3(W, y, s),
            Wall::Front => v3(s, y, 0.0),
            Wall::Back => v3(s, y, D),
        }
    }
    fn normal(self) -> Vec3 {
        match self {
            Wall::Left => Vec3::X,
            Wall::Right => -Vec3::X,
            Wall::Front => Vec3::Z,
            Wall::Back => -Vec3::Z,
        }
    }
}

fn landing_y(n: usize) -> f32 {
    n as f32 * H
}
fn mid_y(n: usize) -> f32 {
    n as f32 * H + HALF
}

/// Высота пола под точкой для ходьбы: марши — ровным пандусом по
/// подступенкам (ноги не цепляются за каждую ступень). Возвращает все
/// поверхности в этой колонке.
pub fn surfaces(x: f32, z: f32) -> Vec<f32> {
    let mut out = Vec::new();
    if z <= LAND_Z {
        for n in 0..FLOORS {
            out.push(landing_y(n));
        }
    } else if z >= MID_Z {
        for n in 0..FLOORS - 1 {
            out.push(mid_y(n));
        }
    } else {
        let t = (z - LAND_Z) / (MID_Z - LAND_Z);
        for n in 0..FLOORS - 1 {
            if x >= (A_X + B_X) * 0.5 {
                out.push(landing_y(n) + t * HALF);
            } else {
                out.push(mid_y(n) + (1.0 - t) * HALF);
            }
        }
    }
    out
}

pub fn build() -> Building {
    let mut b = Building::empty();
    walls(&mut b);
    repaint_patches(&mut b);
    slabs_and_flights(&mut b);
    railings(&mut b);
    doors_and_fittings(&mut b);
    belongings(&mut b);
    gas_pipe(&mut b);
    wet(&mut b);
    windows(&mut b);
    lamps(&mut b);
    wear_decals(&mut b);
    // группы видимости: что строится дальше — уже не клетка
    let interior = std::mem::replace(&mut b.geo, Building::no_geo());
    yard(&mut b);
    b.back = std::mem::replace(&mut b.geo, Building::no_geo());
    front_yard(&mut b);
    b.front = std::mem::replace(&mut b.geo, interior);
    b
}

/// Закрашенные надписи: ЖЭК замазывает граффити прямоугольниками краски —
/// оттенок чуть другой, свежая краска глаже; иногда сквозь проступает.
fn repaint_patches(b: &mut Building) {
    let spots: [(Wall, f32, f32, f32, f32, [f32; 3]); 7] = [
        (Wall::Left, 1.0, landing_y(1) + 0.4, 0.5, 0.35, [0.92, 1.0, 0.96]),
        (Wall::Right, 2.7, landing_y(0) + 0.9, 0.7, 0.4, [1.06, 1.04, 1.0]),
        (Wall::Back, 0.3, mid_y(1) + 0.5, 0.35, 0.5, [0.9, 0.96, 1.02]),
        (Wall::Right, 3.3, landing_y(2) + 1.05, 0.55, 0.3, [0.95, 1.05, 0.98]),
        (Wall::Left, 2.9, mid_y(2) + 0.9, 0.6, 0.35, [1.05, 1.0, 0.95]),
        (Wall::Front, 0.35, landing_y(3) + 0.6, 0.3, 0.45, [0.93, 0.98, 1.0]),
        (Wall::Back, 2.2, mid_y(3) + 0.35, 0.4, 0.4, [1.04, 1.02, 0.97]),
    ];
    for (wall, s, y, w, h, tint) in spots {
        let n = wall.normal();
        let p = |a: f32, yy: f32| wall.at(a, yy) + n * 0.0015;
        // u — как у стены (s / 2), v — фрагмент тайла краски без полосы сверху
        let uv = [[(s - w / 2.0) / 2.0, 0.9], [(s + w / 2.0) / 2.0, 0.9], [(s + w / 2.0) / 2.0, 0.9 - h / 1.5], [(s - w / 2.0) / 2.0, 0.9 - h / 1.5]];
        b.quad4(Mat::Paint, [p(s - w / 2.0, y), p(s + w / 2.0, y), p(s + w / 2.0, y + h), p(s - w / 2.0, y + h)], uv, n, tint);
    }
}

fn walls(b: &mut Building) {
    // низ полосы — внутри плиты перекрытия (20 см), иначе краска следующего
    // этажа торчит каймой из-под потолка
    let lo = -0.19;
    let last = FLOORS - 1;
    // Левая стена (x = 0): площадка k → марш B(k-1) вниз → промежуточная k-1.
    for k in 0..FLOORS {
        let y = landing_y(k);
        let top = (k == last).then_some(ROOF);
        b.wall(Wall::Left, 0.0, LAND_Z, y, y, lo, H, top);
        if k >= 1 {
            b.wall(Wall::Left, LAND_Z, MID_Z, y, mid_y(k - 1), lo, H, top);
            b.wall(Wall::Left, MID_Z, D, mid_y(k - 1), mid_y(k - 1), lo, H, top);
        }
    }
    // Правая (x = W): площадка k → марш A(k) вверх → промежуточная k.
    for k in 0..FLOORS {
        let y = landing_y(k);
        b.wall(Wall::Right, 0.0, LAND_Z, y, y, lo, H, (k == last).then_some(ROOF));
        if k < last {
            let top = (k == last - 1).then_some(ROOF);
            b.wall(Wall::Right, LAND_Z, MID_Z, y, mid_y(k), lo, H, top);
            b.wall(Wall::Right, MID_Z, D, mid_y(k), mid_y(k), lo, H, top);
        }
    }
    // Передняя (z = 0): только площадки. ИЗМЕНЕНО (выход на улицу): на
    // первом этаже — проём входной двери (рама DOOR_X0−0.07..DOOR_X1+0.07).
    for k in 0..FLOORS {
        let y = landing_y(k);
        let top = (k == last).then_some(ROOF);
        if k == 0 {
            let (a, c) = (DOOR_X0 - 0.07, DOOR_X1 + 0.07);
            b.wall(Wall::Front, 0.0, a, y, y, lo, H, top);
            b.wall(Wall::Front, c, W, y, y, lo, H, top);
            b.wall(Wall::Front, a, c, y, y, 2.17, H, top);
        } else {
            b.wall(Wall::Front, 0.0, W, y, y, lo, H, top);
        }
    }
    // Задняя (z = D): промежуточные площадки с проёмом окна.
    for k in 0..FLOORS - 1 {
        let y = mid_y(k);
        let top = (k == last - 1).then_some(ROOF);
        b.wall(Wall::Back, 0.0, WIN_X[0], y, y, lo, H, top);
        b.wall(Wall::Back, WIN_X[1], W, y, y, lo, H, top);
        b.wall(Wall::Back, WIN_X[0], WIN_X[1], y, y, lo, WIN_Y[0], None);
        b.wall(Wall::Back, WIN_X[0], WIN_X[1], y, y, WIN_Y[1], H, top);
    }
    // Кладовка под первым маршем B: перегородка (как кусок передней стены,
    // перенесённый на z = 1.6) с дверцей.
    let mark = wall_mark(b);
    b.wall(Wall::Front, 0.0, B_X, 0.0, 0.0, 0.0, 2.55, None);
    shift_wall(b, mark, 1.6);
    // левая стена первого этажа до перегородки (иначе щель в кладовку)
    b.wall(Wall::Left, LAND_Z, 1.6, 0.0, 0.0, -0.19, 2.66, None);
    // дверца: вертикальные доски со щелями, планки-«зет», петли, замок
    let brown = [0.13, 0.065, 0.035];
    let zf = 1.6 - 0.004;
    for k in 0..5 {
        let x0 = 0.25 + k as f32 * 0.14;
        b.cube(Mat::Wood, v3(x0 + 0.002, 0.03, zf - 0.025), v3(x0 + 0.138, 1.75, zf), 0.5, [brown[0] * (0.9 + 0.05 * k as f32), brown[1], brown[2]]);
    }
    for y in [0.25f32, 1.5] {
        b.cube(Mat::Wood, v3(0.27, y, zf - 0.045), v3(0.93, y + 0.1, zf - 0.025), 0.5, brown);
    }
    b.beam(Mat::Wood, v3(0.3, 0.35, zf - 0.035), v3(0.9, 1.5, zf - 0.035), Vec3::Z, 0.02, 0.09, brown);
    for y in [0.3f32, 1.45] {
        b.cube(Mat::Black, v3(0.2, y, zf - 0.05), v3(0.45, y + 0.035, zf - 0.044), 0.1, [0.08; 3]);
    }
    b.cube(Mat::Metal, v3(0.86, 0.95, zf - 0.04), v3(1.02, 1.0, zf - 0.03), 0.5, [0.3, 0.3, 0.3]);
    b.cube(Mat::Brass, v3(0.94, 0.86, zf - 0.07), v3(0.99, 0.93, zf - 0.045), 0.1, [0.7; 3]);
    b.cyl(Mat::Metal, v3(0.965, 0.93, zf - 0.058), 0.025, 0.012, 8, [0.5; 3]);
    // объявления (лист А4, чуть косо) у входа и на дверце
    paper(b, Wall::Front, 2.15, 1.95, 0.0, 0.03, 0.06);
    paper(b, Wall::Front, 0.35, 1.55, 0.0, -0.05, 0.0);
    paper(b, Wall::Front, 2.2, 0.75, 0.0, 0.02, 0.12);
    for n in 1..FLOORS {
        // на щитке — листок «Показания счётчиков сдавать до 25-го»
        if n % 2 == 1 {
            let y = landing_y(n);
            b.quad4(Mat::Paper, [v3(1.98, y + 1.55, 0.142), v3(2.2, y + 1.55, 0.142), v3(2.2, y + 1.85, 0.142), v3(1.98, y + 1.85, 0.142)], [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], Vec3::Z, [0.95, 0.93, 0.85]);
        }
    }
    // её боковая стенка вдоль просвета (под маршем B) до его низа
    let soffit = |z: f32| mid_y(0) - SLAB + (MID_Z - z) * HALF / (MID_Z - LAND_Z);
    let (z0, z1) = (1.6, MID_Z);
    let paint_top = soffit(z1);
    let pv = |y: f32| (PAINT_H - y) / PAINT_H;
    b.quad4(Mat::Paint, [v3(B_X, 0.0, z0), v3(B_X, 0.0, z1), v3(B_X, paint_top, z1), v3(B_X, paint_top, z0)], [[z0 / 2.0, pv(0.0)], [z1 / 2.0, pv(0.0)], [z1 / 2.0, pv(paint_top)], [z0 / 2.0, pv(paint_top)]], Vec3::X, [1.0; 3]);
    b.quad_w(Mat::White, [v3(B_X, paint_top, z0), v3(B_X, paint_top, z1), v3(B_X, soffit(z1), z1), v3(B_X, soffit(z0), z0)], Vec3::X, 2.0, [1.0; 3]);
    b.quad_w(Mat::Concrete, [v3(B_X, 0.0, LAND_Z), v3(A_X, 0.0, LAND_Z), v3(A_X, 0.0, MID_Z), v3(B_X, 0.0, MID_Z)], Vec3::Y, 1.0, [0.7; 3]);
}

/// Лист А4 на стене `wall`: центр (s вдоль стены, y), наклон `tilt` рад,
/// `age` 0..1 — пожелтел и посерел.
fn paper(b: &mut Building, wall: Wall, s: f32, y: f32, floor: f32, tilt: f32, age: f32) {
    let n = wall.normal();
    let along = match wall {
        Wall::Left | Wall::Right => Vec3::Z,
        _ => Vec3::X,
    };
    let (hw, hh) = (0.105, 0.1485);
    let (c, sn) = (tilt.cos(), tilt.sin());
    let u = along * c + Vec3::Y * sn;
    let v = -along * sn + Vec3::Y * c;
    let o = wall.at(s, floor + y) + n * 0.003;
    let col = [1.0 - age * 0.15, 0.98 - age * 0.22, 0.92 - age * 0.4];
    b.quad4(Mat::Paper, [o - u * hw - v * hh, o + u * hw - v * hh, o + u * hw + v * hh, o - u * hw + v * hh], [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], n, col);
}

/// Сколько вершин сейчас в краске и побелке — отметка для `shift_wall`.
fn wall_mark(b: &mut Building) -> [usize; 2] {
    [b.g(Mat::Paint).v.len(), b.g(Mat::White).v.len()]
}

/// Перенести стену, построенную после отметки `mark` (краска и побелка
/// перегородки), с z = 0 на z = `z` и развернуть лицом к площадке:
/// отражение по z — прогиб штукатурки остаётся вглубь стены.
fn shift_wall(b: &mut Building, mark: [usize; 2], z: f32) {
    for (m, from) in [Mat::Paint, Mat::White].into_iter().zip(mark) {
        for v in &mut b.g(m).v[from..] {
            v.position[2] = z - v.position[2];
            v.normal[2] = -v.normal[2];
        }
    }
}

fn slabs_and_flights(b: &mut Building) {
    let white = [1.0; 3];
    let up = Vec3::Y;
    let down = -Vec3::Y;
    for n in 0..FLOORS {
        let y = landing_y(n);
        // пол площадки — плитка
        dirty_floor(b, 0.0, W, 0.0, LAND_Z, y);
        // потолок под ней (кроме первого этажа)
        if n >= 1 {
            let yb = y - SLAB;
            b.quad_w(Mat::White, [v3(0.0, yb, 0.0), v3(W, yb, 0.0), v3(W, yb, LAND_Z), v3(0.0, yb, LAND_Z)], down, 2.0, white);
            // торец плиты в просвете
            b.quad_w(Mat::Concrete, [v3(B_X, yb, LAND_Z), v3(A_X, yb, LAND_Z), v3(A_X, y, LAND_Z), v3(B_X, y, LAND_Z)], Vec3::Z, 1.0, white);
        }
    }
    // торец верхней площадки над пустотой марша A
    let y4 = landing_y(FLOORS - 1);
    b.quad_w(Mat::Concrete, [v3(A_X, y4 - SLAB, LAND_Z), v3(W, y4 - SLAB, LAND_Z), v3(W, y4, LAND_Z), v3(A_X, y4, LAND_Z)], Vec3::Z, 1.0, white);
    // потолок верхнего этажа — плита покрытия по всей клетке
    b.quad_w(Mat::White, [v3(0.0, ROOF, 0.0), v3(W, ROOF, 0.0), v3(W, ROOF, D), v3(0.0, ROOF, D)], down, 2.0, white);

    for n in 0..FLOORS - 1 {
        let ym = mid_y(n);
        dirty_floor(b, 0.0, W, MID_Z, D, ym);
        if n >= 1 {
            let yb = ym - SLAB;
            b.quad_w(Mat::White, [v3(0.0, yb, MID_Z), v3(W, yb, MID_Z), v3(W, yb, D), v3(0.0, yb, D)], down, 2.0, white);
        }
        b.quad_w(Mat::Concrete, [v3(B_X, ym - SLAB, MID_Z), v3(A_X, ym - SLAB, MID_Z), v3(A_X, ym, MID_Z), v3(B_X, ym, MID_Z)], -Vec3::Z, 1.0, white);

        let yl = landing_y(n);
        let slope = (HALF) / (MID_Z - LAND_Z);
        // ---- марш A: вверх по +Z, x A_X..W ----
        let soffit_a = |z: f32| if n == 0 { 0.0 } else { yl - SLAB + (z - LAND_Z) * slope };
        for k in 1..=RISERS {
            let z0 = LAND_Z + (k - 1) as f32 * RUN;
            let (ylo, yhi) = (yl + (k - 1) as f32 * RISE, yl + k as f32 * RISE);
            // подступенок
            b.quad_w(Mat::Concrete, [v3(A_X, ylo, z0), v3(W, ylo, z0), v3(W, yhi, z0), v3(A_X, yhi, z0)], -Vec3::Z, 1.0, white);
            if k < RISERS {
                let z1 = z0 + RUN;
                // проступь: u поперёк марша, v от носка
                let (uv, tone) = tread_uv(n, 0, k);
                b.quad4(Mat::Tread, [v3(A_X, yhi, z0), v3(W, yhi, z0), v3(W, yhi, z1), v3(A_X, yhi, z1)], uv, up, tone);
                // носок скруглён и затёрт (бетон не держит острую кромку)
                b.tube(Mat::Concrete, v3(A_X, yhi - 0.012, z0 + 0.012), v3(W, yhi - 0.012, z0 + 0.012), 0.0135, 1.0, 8, [0.92; 3]);
                // бок ступени в просвет
                b.quad_w(Mat::Concrete, [v3(A_X, soffit_a(z0).min(ylo), z0), v3(A_X, soffit_a(z1).min(yhi), z1), v3(A_X, yhi, z1), v3(A_X, yhi, z0)], -Vec3::X, 1.0, white);
            }
        }
        if n >= 1 {
            let (s0, s1) = (yl - SLAB, ym - SLAB);
            let nn = v3(0.0, -1.0, slope).normalize();
            b.quad_w(Mat::White, [v3(A_X, s0, LAND_Z), v3(W, s0, LAND_Z), v3(W, s1, MID_Z), v3(A_X, s1, MID_Z)], nn, 2.0, white);
        }
        // ---- марш B: вверх по −Z, x 0..B_X ----
        let soffit_b = |z: f32| ym - SLAB + (MID_Z - z) * slope;
        for k in 1..=RISERS {
            let z0 = MID_Z - (k - 1) as f32 * RUN;
            let (ylo, yhi) = (ym + (k - 1) as f32 * RISE, ym + k as f32 * RISE);
            b.quad_w(Mat::Concrete, [v3(0.0, ylo, z0), v3(B_X, ylo, z0), v3(B_X, yhi, z0), v3(0.0, yhi, z0)], Vec3::Z, 1.0, white);
            if k < RISERS {
                let z1 = z0 - RUN;
                let (uv, tone) = tread_uv(n, 1, k);
                b.quad4(Mat::Tread, [v3(B_X, yhi, z0), v3(0.0, yhi, z0), v3(0.0, yhi, z1), v3(B_X, yhi, z1)], uv, up, tone);
                b.tube(Mat::Concrete, v3(0.0, yhi - 0.012, z0 - 0.012), v3(B_X, yhi - 0.012, z0 - 0.012), 0.0135, 1.0, 8, [0.92; 3]);
                b.quad_w(Mat::Concrete, [v3(B_X, soffit_b(z0).min(ylo), z0), v3(B_X, soffit_b(z1).min(yhi), z1), v3(B_X, yhi, z1), v3(B_X, yhi, z0)], Vec3::X, 1.0, white);
            }
        }
        let (s0, s1) = (ym - SLAB, yl + H - SLAB);
        let nn = v3(0.0, -1.0, -slope).normalize();
        b.quad_w(Mat::White, [v3(0.0, s0, MID_Z), v3(B_X, s0, MID_Z), v3(B_X, s1, LAND_Z), v3(0.0, s1, LAND_Z)], nn, 2.0, white);
    }
}

/// Проступь ступени `k` (1..RISERS-1) марша `flight` (0 — A, 1 — B) между
/// этажами n+1 и n+2: ячейка атласа `tex::treads` и тон бетона этой отливки.
/// Износ — по числу этажей, чьи жильцы по маршу ходят (на первом — все
/// четыре верхних, на последнем — только пятый); первая и последняя ступени
/// стёрты сильнее (с площадки шагают на них, сбивая носок). Отливка
/// случайная, но не как у ступени ниже — рядом одинаковых нет.
fn tread_uv(n: usize, flight: usize, k: usize) -> ([[f32; 2]; 4], [f32; 3]) {
    use crate::tex::{TREAD_CASTS, TREAD_LEVELS};
    let traffic = (FLOORS - 2).saturating_sub(n);
    let level = (traffic + (k == 1 || k == RISERS - 1) as usize).min(TREAD_LEVELS - 1);
    let id = (n * 2 + flight) as i32;
    let cast_of = |k: usize| (hash01(id, k as i32 + 101) * TREAD_CASTS as f32) as usize % TREAD_CASTS;
    let mut cast = cast_of(k);
    if k > 1 && cast == cast_of(k - 1) {
        cast = (cast + 1) % TREAD_CASTS;
    }
    // ячейка 512 текселей: полтора от края — фильтр не берёт соседнюю
    let e = 1.5 / 512.0;
    let (cu, cv) = (cast as f32, level as f32);
    let (u0, u1) = ((cu + e) / TREAD_CASTS as f32, (cu + 1.0 - e) / TREAD_CASTS as f32);
    let (v0, v1) = ((cv + e) / TREAD_LEVELS as f32, (cv + 1.0 - e) / TREAD_LEVELS as f32);
    let t = 0.95 + 0.1 * hash01(id, k as i32 + 211);
    ([[u0, v0], [u1, v0], [u1, v1], [u0, v1]], [t, t * 0.99, t * 0.98])
}

/// Плитка площадки сеткой 10 см: у стен и в углах, куда не ступают, —
/// слой пыли и грязи (тон вершин темнее), посередине протёрто ногами.
fn dirty_floor(b: &mut Building, x0: f32, x1: f32, z0: f32, z1: f32, y: f32) {
    let step = 0.1;
    let (nx, nz) = (((x1 - x0) / step).round() as usize, ((z1 - z0) / step).round() as usize);
    let tone = |x: f32, z: f32| {
        let d = (x - x0).min(x1 - x).min(z - z0).min(z1 - z);
        let edge = (1.0 - (d / 0.35).clamp(0.0, 1.0)).powi(2);
        let n = hash01((x * 10.0) as i32, (z * 10.0) as i32);
        let k = 1.0 - 0.45 * edge - 0.05 * n;
        [k, k * 0.98, k * 0.95]
    };
    for i in 0..nx {
        for j in 0..nz {
            let (xa, xb) = (x0 + i as f32 * (x1 - x0) / nx as f32, x0 + (i + 1) as f32 * (x1 - x0) / nx as f32);
            let (za, zb) = (z0 + j as f32 * (z1 - z0) / nz as f32, z0 + (j + 1) as f32 * (z1 - z0) / nz as f32);
            let p = [v3(xa, y, za), v3(xb, y, za), v3(xb, y, zb), v3(xa, y, zb)];
            let uv = p.map(|q| [q.x / 0.6, q.z / 0.6]);
            // тон — по центру ячейки (вершины ячеек не общие, поэтому без
            // интерполяции; ячейка 10 см — ступенек не видно)
            let c = tone((xa + xb) * 0.5, (za + zb) * 0.5);
            b.quad4(Mat::Tiles, p, uv, Vec3::Y, c);
        }
    }
}

fn railings(b: &mut Building) {
    let paint = [0.20, 0.17, 0.15];
    let rail_h = 0.9;
    let slope = HALF / (MID_Z - LAND_Z);
    for n in 0..FLOORS - 1 {
        let yl = landing_y(n);
        let ym = mid_y(n);
        // (x перил, z начала, высота носка в начале, направление по z)
        for (x, z_start, y_start, dir) in [(A_X + 0.03, LAND_Z, yl, 1.0f32), (B_X - 0.03, MID_Z, ym, -1.0f32)] {
            let at = |s: f32| (z_start + dir * s, y_start + s * slope);
            for k in 0..RISERS - 1 {
                let s = (k as f32 + 0.5) * RUN;
                let (z, _) = at(s);
                let tread_y = y_start + (k + 1) as f32 * RISE;
                let (_, hy) = at(s);
                b.cube(Mat::Metal, v3(x - 0.008, tread_y, z - 0.008), v3(x + 0.008, hy + rail_h - 0.03, z + 0.008), 0.5, paint);
            }
            let (z0, y0) = at(0.1);
            let (z1, y1) = at(MID_Z - LAND_Z - 0.1);
            // нижняя полоса и поручень
            b.beam(Mat::Metal, v3(x, y0 + 0.12, z0), v3(x, y1 + 0.12, z1), Vec3::X, 0.008, 0.04, paint);
            b.beam(Mat::Metal, v3(x, y0 + rail_h - 0.03, z0), v3(x, y1 + rail_h - 0.03, z1), Vec3::X, 0.04, 0.008, paint);
            b.tube(Mat::Rail, v3(x, y0 + rail_h, z0), v3(x, y1 + rail_h, z1), 0.03, 0.6, 12, [1.0; 3]);
        }
        // поворот поручня: концы поручней двух маршей выходят чуть на
        // площадку и соединяются коротким отрезком через просвет — на своих
        // высотах (у верхнего марша поручень выше на подступенок)
        let ra = MID_Z - LAND_Z - 0.1;
        let turn = |b: &mut Building, xa: f32, ya: f32, xb: f32, yb: f32, z: f32, zt: f32| {
            let (pa, pb) = (v3(xa, ya, z), v3(xb, yb, z));
            let (ta, tb) = (v3(xa, ya, zt), v3(xb, yb, zt));
            b.tube(Mat::Rail, pa, ta, 0.03, 0.6, 12, [1.0; 3]);
            b.tube(Mat::Rail, pb, tb, 0.03, 0.6, 12, [1.0; 3]);
            b.tube(Mat::Rail, ta - (tb - ta).normalize_or_zero() * 0.02, tb + (tb - ta).normalize_or_zero() * 0.02, 0.03, 0.6, 12, [1.0; 3]);
        };
        // на промежуточной: конец A(n) и начало B(n)
        turn(b, A_X + 0.03, yl + ra * slope + rail_h, B_X - 0.03, ym + 0.1 * slope + rail_h, MID_Z - 0.1, MID_Z + 0.12);
        // на этажной n+1: конец B(n) и начало A(n+1) (на верхней — ограждение)
        let yn = landing_y(n + 1);
        let a_next = if n + 1 < FLOORS - 1 { yn + 0.1 * slope + rail_h } else { yn + rail_h };
        turn(b, B_X - 0.03, ym + ra * slope + rail_h, A_X + 0.03, a_next, LAND_Z + 0.1, LAND_Z - 0.03);
    }
    // ограждение верхней площадки над маршем A
    let y4 = landing_y(FLOORS - 1);
    let z = LAND_Z - 0.03;
    let mut x = A_X + 0.1;
    while x < W - 0.05 {
        b.cube(Mat::Metal, v3(x - 0.008, y4, z - 0.008), v3(x + 0.008, y4 + rail_h - 0.03, z + 0.008), 0.5, paint);
        x += 0.15;
    }
    b.beam(Mat::Metal, v3(A_X, y4 + 0.12, z), v3(W, y4 + 0.12, z), Vec3::Z, 0.008, 0.04, paint);
    b.tube(Mat::Rail, v3(B_X - 0.03, y4 + rail_h, z), v3(W, y4 + rail_h, z), 0.03, 0.6, 12, [1.0; 3]);
}

/// Дверь квартиры в стене `wall` с центром `s` вдоль неё: полотно
/// (дермантин или крашеный металл), наличники, ручка, глазок.
fn door(b: &mut Building, wall: Wall, s: f32, floor: f32, kind: u32, number: u32) {
    let n = wall.normal();
    let along = match wall {
        Wall::Left | Wall::Right => Vec3::Z,
        _ => Vec3::X,
    };
    let (w, h) = (0.85, 2.0);
    let base = wall.at(s, floor) + n * 0.035;
    let p = |a: f32, y: f32| base + along * a + Vec3::Y * y;
    let corners = [p(-w / 2.0, 0.0), p(w / 2.0, 0.0), p(w / 2.0, h), p(-w / 2.0, h)];
    // u вдоль полотна, v сверху вниз
    let uv = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
    let tints = [[1.0, 1.0, 1.0], [0.55, 0.5, 0.5], [1.25, 0.85, 0.75], [0.9, 1.0, 0.85]];
    if kind % 4 == 3 {
        // металлическая дверь 90-х, крашеная
        b.quad4(Mat::Metal, corners, uv, n, [0.32, 0.2, 0.12]);
    } else {
        b.quad4(Mat::Door, corners, uv, n, tints[kind as usize % 3]);
    }
    // торцы полотна
    let off = -n * 0.035;
    for (a0, a1) in [(corners[1], corners[2]), (corners[3], corners[0])] {
        b.quad_w(Mat::Black, [a0, a1, a1 + off, a0 + off], along, 0.5, [0.3; 3]);
    }
    // коврик у двери — у каждой свой, чуть сбитый набок
    let rug_tints = [[1.0, 1.0, 1.0], [0.8, 0.9, 1.1], [1.1, 0.85, 0.7], [0.7, 0.7, 0.7]];
    let skew = (kind as f32 * 1.7).sin() * 0.08;
    let rc = wall.at(s, floor) + n * (0.32 + skew.abs()) + along * skew + Vec3::Y * 0.001;
    let (ra, rn) = (along * 0.34, n * 0.22);
    let ry = Vec3::Y * 0.012;
    let rt = rug_tints[kind as usize % 4];
    b.quad4(Mat::Rug, [rc - ra - rn + ry, rc + ra - rn + ry, rc + ra + rn + ry, rc - ra + rn + ry], [[0.0, 0.0], [1.0, 0.0], [1.0, 0.65], [0.0, 0.65]], Vec3::Y, rt);
    for (e0, e1) in [(-ra - rn, ra - rn), (ra - rn, ra + rn), (ra + rn, -ra + rn), (-ra + rn, -ra - rn)] {
        let nn = (e0 + e1).normalize();
        b.quad_w(Mat::Rug, [rc + e0, rc + e1, rc + e1 + ry, rc + e0 + ry], nn, 0.3, [rt[0] * 0.6, rt[1] * 0.6, rt[2] * 0.6]);
    }
    // наличники
    let frame = [0.84, 0.8, 0.7];
    let fw = 0.07;
    let wall_p = |a: f32, y: f32, d: f32| wall.at(s, floor) + along * a + Vec3::Y * y + n * d;
    let fb = |b: &mut Building, a0: f32, a1: f32, y0: f32, y1: f32| {
        let lo = wall_p(a0, y0, 0.0);
        let hi = wall_p(a1, y1, 0.02);
        b.cube(Mat::Wood, lo.min(hi), lo.max(hi), 0.5, frame);
    };
    fb(b, -w / 2.0 - fw, -w / 2.0, 0.0, h + fw);
    fb(b, w / 2.0, w / 2.0 + fw, 0.0, h + fw);
    fb(b, -w / 2.0 - fw, w / 2.0 + fw, h, h + fw);
    // ручка и глазок
    let side = if kind % 2 == 0 { 1.0 } else { -1.0 };
    // нажимная ручка: круглая розетка, шейка и рычаг к середине полотна
    let hp = p(side * (w / 2.0 - 0.08), 1.0);
    b.tube(Mat::Brass, hp, hp + n * 0.008, 0.026, 1.0, 16, [1.0; 3]);
    b.tube(Mat::Brass, hp + n * 0.008, hp + n * 0.05, 0.009, 1.0, 10, [1.0; 3]);
    let tip = hp + n * 0.05 - along * (side * 0.12) - Vec3::Y * 0.01;
    b.tube(Mat::Brass, hp + n * 0.05, tip, 0.009, 1.0, 10, [1.0; 3]);
    // звонок на стене со стороны ручки: белая коробочка и чёрная кнопка
    let bell = wall.at(s, floor) + along * (side * (w / 2.0 + 0.16)) + Vec3::Y * 1.5;
    let bs = v3(0.025, 0.035, 0.025) - n.abs() * 0.012;
    b.cube(Mat::Metal, bell - bs, bell + bs + n * 0.0, 0.5, [0.85, 0.84, 0.8]);
    b.tube(Mat::Black, bell + n * 0.012, bell + n * 0.02, 0.008, 1.0, 10, [0.02; 3]);
    // личинка замка под ручкой: латунный цилиндр в полотне
    let lock = p(side * (w / 2.0 - 0.08), 0.88);
    b.tube(Mat::Brass, lock, lock + n * 0.012, 0.011, 1.0, 12, [0.9; 3]);
    b.cube(Mat::Black, lock + n * 0.012 - Vec3::Y * 0.006 - along.abs() * 0.0015, lock + n * 0.0125 + Vec3::Y * 0.006 + along.abs() * 0.0015, 0.1, [0.02; 3]);
    // глазок: круглая латунная оправа и тёмная выпуклая линза
    let pp = p(0.0, 1.55);
    b.tube(Mat::Brass, pp - n * 0.002, pp + n * 0.008, 0.011, 1.0, 14, [1.0; 3]);
    b.tube(Mat::Black, pp + n * 0.008, pp + n * 0.0095, 0.0065, 1.0, 14, [0.01; 3]);
    // номерок: латунная табличка и выбитые цифры
    let np = p(0.0, 1.78) + n * 0.003;
    let ns = v3(0.003, 0.03, 0.003) + along.abs() * 0.04;
    b.cube(Mat::Brass, np - ns, np + ns, 0.1, [0.8; 3]);
    let text = number.to_string();
    let (dw, dh) = (0.024, 0.04);
    let x0 = -(text.len() as f32) * dw * 0.5;
    for (k, ch) in text.chars().enumerate() {
        let dgt = ch.to_digit(10).unwrap_or(0) as f32;
        let c = np + n * 0.0035 + along * (x0 + (k as f32 + 0.5) * dw);
        let (hu, hv) = (along * (dw * 0.5), Vec3::Y * (dh * 0.5));
        let (u0, u1) = (dgt / 10.0, (dgt + 1.0) / 10.0);
        // u вдоль «along» — для стен, смотрящих в −X/−Z, цифры зеркальны:
        // разворачиваем так, чтобы читались снаружи
        let flip = n.dot(along.cross(Vec3::Y)) < 0.0;
        let (ua, ub) = if flip { (u1, u0) } else { (u0, u1) };
        b.quad4(Mat::Digits, [c - hu - hv, c + hu - hv, c + hu + hv, c - hu + hv], [[ua, 1.0], [ub, 1.0], [ub, 0.0], [ua, 0.0]], n, [1.0; 3]);
    }
}

fn doors_and_fittings(b: &mut Building) {
    let mut kind = 0;
    for n in 0..FLOORS {
        let y = landing_y(n);
        let first = if n == 0 { 1 } else { 3 * n as u32 };
        door(b, Wall::Left, 0.75, y, kind, first);
        kind += 1;
        door(b, Wall::Right, 0.75, y, kind + 1, if n == 0 { 2 } else { first + 2 });
        kind += 2;
        if n == 0 {
            // входная дверь подъезда: доски, крашенные коричневым
            let (x0, x1) = (DOOR_X0, DOOR_X1);
            // ИЗМЕНЕНО (выход на улицу): полотно — в b.door, координаты от
            // петли: дверь поворачивается наружу (как у подъездов), рама и
            // доводчик остаются в стене
            let mut d = Building::empty();
            // полотно из вертикальных досок (фаски — тёмные щели между ними)
            for k in 0..8 {
                let bx = x0 + k as f32 * 0.125;
                let tone = 0.9 + 0.12 * ((k * 37 % 7) as f32 / 7.0);
                d.cube(Mat::Wood, v3(bx + 0.003, 0.0, 0.0), v3(bx + 0.122, 2.1, 0.05), 0.5, [0.12 * tone, 0.06 * tone, 0.035 * tone]);
            }
            d.cube(Mat::Black, v3(x0, 0.0, 0.0), v3(x1, 2.1, 0.045), 0.1, [0.02; 3]);
            // металлическая полоса снизу (от пинков) и скоба-ручка
            d.cube(Mat::Metal, v3(x0, 0.0, 0.05), v3(x1, 0.3, 0.054), 0.5, [0.25, 0.24, 0.22]);
            d.tube(Mat::Metal, v3(x1 - 0.1, 0.95, 0.12), v3(x1 - 0.1, 1.25, 0.12), 0.012, 1.0, 10, [0.3; 3]);
            d.cube(Mat::Metal, v3(x1 - 0.112, 0.94, 0.05), v3(x1 - 0.088, 0.96, 0.13), 0.5, [0.3; 3]);
            d.cube(Mat::Metal, v3(x1 - 0.112, 1.24, 0.05), v3(x1 - 0.088, 1.26, 0.13), 0.5, [0.3; 3]);
            for g in &mut d.geo {
                for v in &mut g.v {
                    v.position[0] -= DOOR_HINGE.x;
                    v.position[1] -= DOOR_HINGE.y;
                    v.position[2] -= DOOR_HINGE.z;
                }
            }
            b.door = d.geo;
            // пружина-доводчик наверху
            b.beam(Mat::Black, v3(x0 + 0.3, 1.95, 0.08), v3(x0 + 0.3, 2.3, 0.02), Vec3::X, 0.03, 0.03, [0.1; 3]);
            b.cube(Mat::Wood, v3(x0 - 0.07, 0.0, 0.0), v3(x0, 2.17, 0.02), 0.5, [0.12, 0.06, 0.035]);
            b.cube(Mat::Wood, v3(x1, 0.0, 0.0), v3(x1 + 0.07, 2.17, 0.02), 0.5, [0.12, 0.06, 0.035]);
            b.cube(Mat::Wood, v3(x0, 2.1, 0.0), v3(x1, 2.17, 0.02), 0.5, [0.12, 0.06, 0.035]);
            // почтовые ящики
            let (mx0, mx1) = (1.92, 2.45);
            b.cube(Mat::Metal, v3(mx0, 1.0, 0.0), v3(mx1, 1.75, 0.2), 0.5, [0.18, 0.3, 0.42]);
            for r in 0..3 {
                for c in 0..2 {
                    let (cx, cy) = (mx0 + 0.04 + c as f32 * 0.25, 1.05 + r as f32 * 0.23);
                    b.cube(Mat::Black, v3(cx, cy + 0.15, 0.2), v3(cx + 0.2, cy + 0.165, 0.205), 0.1, [0.1; 3]);
                    // контур дверцы ячейки
                    for (a, bb) in [(v3(cx - 0.01, cy - 0.01, 0.2), v3(cx + 0.21, cy - 0.006, 0.2025)), (v3(cx - 0.01, cy + 0.2, 0.2), v3(cx + 0.21, cy + 0.204, 0.2025)), (v3(cx - 0.01, cy - 0.01, 0.2), v3(cx - 0.006, cy + 0.204, 0.2025)), (v3(cx + 0.206, cy - 0.01, 0.2), v3(cx + 0.21, cy + 0.204, 0.2025))] {
                        b.cube(Mat::Black, a, bb, 0.1, [0.02; 3]);
                    }
                    // номер квартиры на дверце ячейки и замочек
                    let num = (2 - r) * 2 + c + 1;
                    let (dw, dh) = (0.016, 0.026);
                    let center = v3(cx + 0.1, cy + 0.09, 0.2015);
                    let text = num.to_string();
                    let x0 = -(text.len() as f32) * dw * 0.5;
                    for (k, ch) in text.chars().enumerate() {
                        let dgt = ch.to_digit(10).unwrap_or(0) as f32;
                        let cc = center + Vec3::X * (x0 + (k as f32 + 0.5) * dw);
                        let (hu, hv) = (Vec3::X * (dw * 0.5), Vec3::Y * (dh * 0.5));
                        b.quad4(Mat::Digits, [cc - hu - hv, cc + hu - hv, cc + hu + hv, cc - hu + hv], [[dgt / 10.0, 1.0], [(dgt + 1.0) / 10.0, 1.0], [(dgt + 1.0) / 10.0, 0.0], [dgt / 10.0, 0.0]], Vec3::Z, [0.9; 3]);
                    }
                    b.cube(Mat::Brass, v3(cx + 0.164, cy + 0.054, 0.2), v3(cx + 0.176, cy + 0.066, 0.204), 0.1, [0.7; 3]);
                }
            }
        } else {
            door(b, Wall::Front, 1.25, y, kind, first + 1);
            kind += 1;
            // электрощиток: корпус, щель дверцы, петли, вентиляционные прорези
            b.cube(Mat::Metal, v3(1.88, y + 1.25, 0.0), v3(2.42, y + 2.05, 0.14), 0.5, [0.42, 0.44, 0.45]);
            b.cube(Mat::Black, v3(1.9, y + 1.27, 0.14), v3(2.4, y + 1.275, 0.1405), 0.1, [0.02; 3]);
            b.cube(Mat::Black, v3(1.9, y + 2.025, 0.14), v3(2.4, y + 2.03, 0.1405), 0.1, [0.02; 3]);
            b.cube(Mat::Black, v3(1.9, y + 1.27, 0.14), v3(1.905, y + 2.03, 0.1405), 0.1, [0.02; 3]);
            for hy in [y + 1.35, y + 1.95] {
                b.cube(Mat::Metal, v3(1.885, hy, 0.135), v3(1.9, hy + 0.06, 0.15), 0.5, [0.3; 3]);
            }
            for k in 0..5 {
                let sy = y + 1.32 + k as f32 * 0.03;
                b.cube(Mat::Black, v3(2.1, sy, 0.14), v3(2.3, sy + 0.012, 0.1405), 0.1, [0.01; 3]);
            }
            // окошко для снятия показаний (внизу слева): за стеклом счётчик —
            // белый циферблат и чёрное окно барабана цифр, рамка — 4 планки
            let (mx, my, mw, mh) = (1.93f32, y + 1.29, 0.15f32, 0.11f32);
            b.quad_w(Mat::Paper, [v3(mx, my, 0.1403), v3(mx + mw, my, 0.1403), v3(mx + mw, my + mh, 0.1403), v3(mx, my + mh, 0.1403)], Vec3::Z, 0.21, [0.8, 0.8, 0.77]);
            b.cube_sharp(Mat::Black, v3(mx + 0.03, my + 0.06, 0.1403), v3(mx + 0.12, my + 0.08, 0.1406), 0.1, [0.01; 3]);
            for (lo, hi) in [(v3(mx - 0.008, my - 0.008, 0.139), v3(mx + mw + 0.008, my, 0.1415)), (v3(mx - 0.008, my + mh, 0.139), v3(mx + mw + 0.008, my + mh + 0.008, 0.1415)), (v3(mx - 0.008, my, 0.139), v3(mx, my + mh, 0.1415)), (v3(mx + mw, my, 0.139), v3(mx + mw + 0.008, my + mh, 0.1415))] {
                b.cube_sharp(Mat::Black, lo, hi, 0.1, [0.03; 3]);
            }
            b.quad_w(Mat::Glass, [v3(mx, my, 0.141), v3(mx + mw, my, 0.141), v3(mx + mw, my + mh, 0.141), v3(mx, my + mh, 0.141)], Vec3::Z, 1.0, [1.0; 3]);
            b.cube(Mat::Black, v3(2.36, y + 1.6, 0.14), v3(2.39, y + 1.7, 0.16), 0.1, [0.2; 3]);
            let (tx, ty, ts) = (2.27, y + 1.92, 0.07);
            b.quad4(Mat::Warning, [v3(tx - ts, ty - ts, 0.1415), v3(tx + ts, ty - ts, 0.1415), v3(tx + ts, ty + ts, 0.1415), v3(tx - ts, ty + ts, 0.1415)], [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], Vec3::Z, [1.0; 3]);
        }
    }
}

/// Талый снег с обуви: лужицы у входной двери и мокрые следы до лестницы и
/// по первым ступеням (вода — гладкая, отражает лампу).
fn wet(b: &mut Building) {
    let up = Vec3::Y;
    let patch = |b: &mut Building, x: f32, y: f32, z: f32, rx: f32, rz: f32, k: u32| {
        let o = 0.0008;
        let p = [v3(x - rx, y + o, z - rz), v3(x + rx, y + o, z - rz), v3(x + rx, y + o, z + rz), v3(x - rx, y + o, z + rz)];
        // разные лужи — разные части текстуры (4 варианта по u, v)
        let (u0, v0) = ((k % 2) as f32 * 0.5, (k / 2 % 2) as f32 * 0.5);
        b.quad4(Mat::Wet, p, [[u0, v0], [u0 + 0.5, v0], [u0 + 0.5, v0 + 0.5], [u0, v0 + 0.5]], up, [1.0; 3]);
    };
    // у входной двери
    patch(b, 1.25, 0.0, 0.32, 0.42, 0.26, 0);
    patch(b, 1.0, 0.0, 0.75, 0.22, 0.16, 1);
    // следы к лестнице (марш A — справа)
    for (k, (x, z)) in [(1.55f32, 0.95f32), (1.8, 1.2), (1.6, 1.38)].into_iter().enumerate() {
        patch(b, x, 0.0, z, 0.09, 0.14, 2 + k as u32);
    }
    // на первых ступенях
    for k in 1..4 {
        let z = LAND_Z + (k as f32 - 0.5) * RUN;
        let x = if k % 2 == 0 { 1.75 } else { 2.05 };
        patch(b, x, k as f32 * RISE, z, 0.085, 0.11, k + 4);
    }
}

/// Газовая труба: жёлтый стояк в углу у входа на площадки (через все
/// этажи), на каждом этаже — отводы под потолком к квартирам и кран.
fn gas_pipe(b: &mut Building) {
    let yellow = [0.62, 0.42, 0.02];
    let (rx, rz) = (W - 0.05, 0.05);
    b.tube(Mat::Metal, v3(rx, 0.3, rz), v3(rx, ROOF - 0.05, rz), 0.017, 1.0, 10, yellow);
    for n in 1..FLOORS {
        let y = landing_y(n) + 2.3;
        // вдоль передней стены к двери посередине, в стену над наличником
        b.tube(Mat::Metal, v3(rx, y, rz), v3(1.62, y, rz), 0.012, 1.0, 8, yellow);
        b.tube(Mat::Metal, v3(1.62, y, rz), v3(1.62, y, 0.0), 0.012, 1.0, 8, yellow);
        // вдоль правой стены к правой двери
        b.tube(Mat::Metal, v3(rx, y - 0.08, rz), v3(rx, y - 0.08, 0.22), 0.012, 1.0, 8, yellow);
        b.tube(Mat::Metal, v3(rx, y - 0.08, 0.22), v3(W, y - 0.08, 0.22), 0.012, 1.0, 8, yellow);
        // кран на отводе: корпус и красная ручка-флажок
        let kx = rx - 0.25;
        b.cube(Mat::Metal, v3(kx - 0.025, y - 0.02, rz - 0.02), v3(kx + 0.025, y + 0.02, rz + 0.02), 0.5, yellow);
        b.cube(Mat::Metal, v3(kx - 0.006, y + 0.02, rz - 0.006), v3(kx + 0.006, y + 0.05, rz + 0.006), 0.5, [0.4; 3]);
        b.cube(Mat::Metal, v3(kx - 0.05, y + 0.05, rz - 0.008), v3(kx + 0.01, y + 0.06, rz + 0.008), 0.5, [0.5, 0.03, 0.02]);
        // хомуты крепления к стене
        for x in [2.1f32, 1.8] {
            b.cube(Mat::Metal, v3(x - 0.01, y - 0.02, 0.0), v3(x + 0.01, y + 0.02, rz + 0.016), 0.5, [0.3; 3]);
        }
    }
}

/// Вещи жильцов: санки у кладовки, мусорный пакет, ботинки на коврике.
fn belongings(b: &mut Building) {
    // детские санки (зима): полозья — гнутая труба, сиденье — рейки,
    // прислонены к перегородке кладовки
    let (sx, sz) = (0.35, 1.38);
    let red = [0.45, 0.05, 0.03];
    for dx in [0.0f32, 0.32] {
        let x = sx + dx;
        // полоз: прямой низ и загнутый вверх нос
        b.tube(Mat::Metal, v3(x, 0.02, sz - 0.3), v3(x, 0.02, sz + 0.12), 0.008, 1.0, 8, red);
        b.tube(Mat::Metal, v3(x, 0.02, sz - 0.3), v3(x, 0.12, sz - 0.4), 0.008, 1.0, 8, red);
        b.tube(Mat::Metal, v3(x, 0.12, sz - 0.4), v3(x, 0.22, sz - 0.34), 0.008, 1.0, 8, red);
        // стойки под сиденье
        for z in [sz - 0.22, sz + 0.05] {
            b.tube(Mat::Metal, v3(x, 0.02, z), v3(x, 0.17, z), 0.007, 1.0, 8, red);
        }
    }
    for k in 0..5 {
        let z = sz - 0.25 + k as f32 * 0.075;
        b.cube(Mat::Wood, v3(sx - 0.03, 0.17, z), v3(sx + 0.35, 0.19, z + 0.055), 0.5, [0.4, 0.22, 0.1]);
    }
    // спинка-ручка
    b.tube(Mat::Metal, v3(sx, 0.17, sz + 0.1), v3(sx, 0.38, sz + 0.16), 0.007, 1.0, 8, red);
    b.tube(Mat::Metal, v3(sx + 0.32, 0.17, sz + 0.1), v3(sx + 0.32, 0.38, sz + 0.16), 0.007, 1.0, 8, red);
    b.tube(Mat::Metal, v3(sx, 0.38, sz + 0.16), v3(sx + 0.32, 0.38, sz + 0.16), 0.007, 1.0, 8, red);

    // мусорный пакет у двери на втором этаже (вынести утром)
    let y2 = landing_y(1);
    let bag = v3(W - 0.25, y2, 1.25);
    let mut prev: Option<[Vec3; 10]> = None;
    let rings = 7;
    for r in 0..=rings {
        let t = r as f32 / rings as f32;
        // тело мешка: пузатое внизу, перетянутое у узла
        let rad = if t < 0.75 { 0.17 * (std::f32::consts::PI * (t / 0.75) * 0.5 + 0.35).sin() + 0.02 } else { 0.03 + 0.05 * (1.0 - t) };
        let h = t * 0.42;
        let ring: [Vec3; 10] = std::array::from_fn(|k| {
            let a = std::f32::consts::TAU * k as f32 / 10.0;
            let wob = 1.0 + 0.08 * ((a * 3.0 + t * 7.0).sin());
            bag + v3(a.cos() * rad * wob, h, a.sin() * rad * wob * 0.85)
        });
        if let Some(pr) = prev {
            for k in 0..10 {
                let k2 = (k + 1) % 10;
                let n = ((pr[k] + pr[k2] + ring[k] + ring[k2]) * 0.25 - (bag + v3(0.0, h, 0.0))).normalize_or_zero();
                b.quad4(Mat::Black, [pr[k], pr[k2], ring[k2], ring[k]], [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], n, [0.015, 0.016, 0.02]);
            }
        }
        prev = Some(ring);
    }
    // ушки узла
    b.beam(Mat::Black, bag + v3(0.0, 0.42, 0.0), bag + v3(-0.05, 0.5, 0.02), Vec3::Z, 0.03, 0.01, [0.015, 0.016, 0.02]);
    b.beam(Mat::Black, bag + v3(0.0, 0.42, 0.0), bag + v3(0.05, 0.49, -0.02), Vec3::Z, 0.03, 0.01, [0.015, 0.016, 0.02]);

    // ботинки на коврике у левой двери третьего этажа
    let y3 = landing_y(2);
    for (k, (x, z, yaw)) in [(0.3f32, 0.55f32, 0.15f32), (0.33, 0.72, -0.1)].into_iter().enumerate() {
        let f = v3(yaw.sin(), 0.0, yaw.cos()) * 0.0;
        let _ = (k, f);
        let base = v3(x, y3 + 0.013, z);
        // подошва (серая резина), носок и голенище — сглаженные тела
        b.cube(Mat::Black, base + v3(-0.048, 0.0, -0.14), base + v3(0.048, 0.022, 0.14), 0.1, [0.12, 0.11, 0.1]);
        b.ellipsoid(Mat::Black, base + v3(0.0, 0.022, -0.04), v3(0.046, 0.065, 0.115), true, [0.02, 0.018, 0.016]);
        b.tube(Mat::Black, base + v3(0.0, 0.022, 0.085), base + v3(0.0, 0.2, 0.095), 0.045, 0.8, 12, [0.02, 0.018, 0.016]);
        // шнурки — светлые полоски поперёк подъёма
        for k in 0..3 {
            let zz = -0.07 + k as f32 * 0.035;
            b.cube_sharp(Mat::Black, base + v3(-0.025, 0.08 - k as f32 * 0.004, zz), base + v3(0.025, 0.084 - k as f32 * 0.004, zz + 0.004), 0.1, [0.25; 3]);
        }
    }
}

fn windows(b: &mut Building) {
    let frame = [0.86, 0.83, 0.74];
    for n in 0..FLOORS - 1 {
        let f = mid_y(n);
        let (x0, x1) = (WIN_X[0], WIN_X[1]);
        let (y0, y1) = (f + WIN_Y[0], f + WIN_Y[1]);
        // откосы
        let dz = WALL_T;
        b.quad_w(Mat::White, [v3(x0, y0, D), v3(x0, y0, D + dz), v3(x0, y1, D + dz), v3(x0, y1, D)], Vec3::X, 2.0, [1.0; 3]);
        b.quad_w(Mat::White, [v3(x1, y0, D), v3(x1, y0, D + dz), v3(x1, y1, D + dz), v3(x1, y1, D)], -Vec3::X, 2.0, [1.0; 3]);
        b.quad_w(Mat::White, [v3(x0, y1, D), v3(x1, y1, D), v3(x1, y1, D + dz), v3(x0, y1, D + dz)], -Vec3::Y, 2.0, [1.0; 3]);
        b.quad_w(Mat::Concrete, [v3(x0, y0, D), v3(x1, y0, D), v3(x1, y0, D + dz), v3(x0, y0, D + dz)], Vec3::Y, 1.0, [1.0; 3]);
        // подоконник
        b.cube(Mat::Wood, v3(x0 - 0.05, y0, D - 0.07), v3(x1 + 0.05, y0 + 0.04, D + 0.14), 0.5, frame);
        // две рамы (внутренняя и наружная) с импостом и форточкой
        for (zf, zg) in [(D + 0.13, D + 0.155), (D + 0.29, D + 0.315)] {
            let t = 0.055;
            let fz = |b: &mut Building, lo: Vec3, hi: Vec3| b.cube(Mat::Wood, lo, hi, 0.5, frame);
            fz(b, v3(x0, y0 + 0.04, zf), v3(x1, y0 + 0.04 + t, zf + 0.05));
            fz(b, v3(x0, y1 - t, zf), v3(x1, y1, zf + 0.05));
            fz(b, v3(x0, y0, zf), v3(x0 + t, y1, zf + 0.05));
            fz(b, v3(x1 - t, y0, zf), v3(x1, y1, zf + 0.05));
            let xm = (x0 + x1) * 0.5;
            fz(b, v3(xm - t * 0.5, y0, zf), v3(xm + t * 0.5, y1, zf + 0.05));
            let yt = y1 - 0.45;
            fz(b, v3(x0, yt - t * 0.5, zf), v3(xm, yt + t * 0.5, zf + 0.05));
            b.quad_w(Mat::Glass, [v3(x0, y0, zg), v3(x1, y0, zg), v3(x1, y1, zg), v3(x0, y1, zg)], -Vec3::Z, 1.0, [1.0; 3]);
        }
        // наружный отлив (жесть) и снег на нём — видно в низ стекла
        b.beam(Mat::Metal, v3(x0 - 0.03, y0 - 0.005, D + 0.34), v3(x1 + 0.03, y0 - 0.005, D + 0.34), Vec3::Z, 0.12, 0.01, [0.45, 0.46, 0.47]);
        b.beam(Mat::Snow, v3(x0 - 0.02, y0 + 0.02, D + 0.35), v3(x1 + 0.02, y0 + 0.02, D + 0.35), Vec3::Z, 0.1, 0.05, [1.0; 3]);
        // морозные узоры на внешнем стекле изнутри (между рамами холодно):
        // больше снизу и по углам; у каждого окна свой кусок текстуры
        let zfr = D + 0.3145 - 0.0015;
        let u0 = (n as f32 * 0.37).fract() * 0.5;
        b.quad4(Mat::Frost, [v3(x0, y0, zfr), v3(x1, y0, zfr), v3(x1, y1, zfr), v3(x0, y1, zfr)], [[u0, 1.0], [u0 + 0.5, 1.0], [u0 + 0.5, 0.0], [u0, 0.0]], -Vec3::Z, [1.0; 3]);
        // надписи маркером по краске у окна (кто-то ждал тут лифта, которого нет)
        let inks = [[0.02, 0.02, 0.025], [0.03, 0.05, 0.25], [0.25, 0.03, 0.03]];
        let ink = inks[n % 3];
        let (sx, sw, sh) = (if n % 2 == 0 { 2.05 } else { 0.08 }, 0.38, 0.22);
        let sy = f + 0.95 + (n as f32 * 0.37).fract() * 0.3;
        b.quad4(Mat::Scribble, [v3(sx, sy, D - 0.002), v3(sx + sw, sy, D - 0.002), v3(sx + sw, sy + sh, D - 0.002), v3(sx, sy + sh, D - 0.002)], [[n as f32 * 0.5, 1.0], [n as f32 * 0.5 + 0.5, 1.0], [n as f32 * 0.5 + 0.5, 0.0], [n as f32 * 0.5, 0.0]], -Vec3::Z, ink);
        // чугунный радиатор под окном: секции
        for k in 0..10 {
            let x = 0.78 + k as f32 * 0.09;
            b.cube(Mat::Metal, v3(x, f + 0.15, D - 0.13), v3(x + 0.07, f + 0.72, D - 0.05), 0.5, [0.8, 0.77, 0.68]);
        }
        b.cube(Mat::Metal, v3(0.74, f + 0.2, D - 0.1), v3(1.72, f + 0.24, D - 0.07), 0.5, [0.8, 0.77, 0.68]);
        b.cube(Mat::Metal, v3(0.74, f + 0.63, D - 0.1), v3(1.72, f + 0.67, D - 0.07), 0.5, [0.8, 0.77, 0.68]);
        // стояк отопления через этаж и подводки к радиатору
        b.cyl(Mat::Metal, v3(0.66, f - 0.2, D - 0.09), H, 0.017, 10, [0.8, 0.77, 0.68]);
        b.cube(Mat::Metal, v3(0.66, f + 0.205, D - 0.1), v3(0.78, f + 0.235, D - 0.07), 0.5, [0.8, 0.77, 0.68]);
        b.cube(Mat::Metal, v3(0.66, f + 0.635, D - 0.1), v3(0.78, f + 0.665, D - 0.07), 0.5, [0.8, 0.77, 0.68]);
        // на подоконнике: банка-пепельница с окурками
        let sill = y0 + 0.04;
        if n != 2 {
            let (cx, cz) = (1.62 - n as f32 * 0.15, D + 0.03);
            b.cyl(Mat::Metal, v3(cx, sill, cz), 0.1, 0.037, 14, [0.55, 0.55, 0.52]);
            for k in 0..(3 + n) {
                let a = k as f32 * 2.1 + n as f32;
                let (ox, oz) = (cx + 0.1 + a.cos() * 0.05, cz + a.sin() * 0.04);
                let d = v3(a.sin(), 0.0, a.cos()) * 0.012;
                let (p0, p1) = (v3(ox, sill, oz) - d, v3(ox, sill + 0.008, oz) + d);
                b.cube(Mat::Wood, p0.min(p1) - v3(0.004, 0.0, 0.004), p0.max(p1) + v3(0.004, 0.0, 0.004), 0.1, [0.9, 0.8, 0.55]);
            }
        } else {
            // горшок с засохшей геранью: глиняный, земля, голые стебли
            let (px, pz) = (1.45, D + 0.04);
            b.cyl(Mat::Concrete, v3(px, sill, pz), 0.13, 0.065, 14, [0.62, 0.3, 0.17]);
            b.cyl(Mat::Black, v3(px, sill + 0.115, pz), 0.0, 0.06, 14, [0.06, 0.045, 0.03]);
            for k in 0..7 {
                let a = k as f32 * 0.9;
                let base = v3(px + a.cos() * 0.02, sill + 0.115, pz + a.sin() * 0.02);
                // засохший стебель: вверх, потом поникшая верхушка
                let mid = base + v3(a.cos() * 0.03, 0.1 + 0.03 * (k % 3) as f32, a.sin() * 0.02);
                let tip = mid + v3(a.cos() * 0.06, -0.02 + 0.01 * (k % 2) as f32, a.sin() * 0.05);
                let col = [0.2, 0.15, 0.07];
                for (p0, p1) in [(base, mid), (mid, tip)] {
                    let side = Vec3::Y.cross((p1 - p0).normalize()).normalize_or(Vec3::X);
                    b.beam(Mat::Wood, p0, p1, side, 0.004, 0.004, col);
                }
            }
        }
    }
}

fn yard(b: &mut Building) {
    let zo = D + WALL_T;
    // снег
    b.quad_w(Mat::Snow, [v3(-40.0, GROUND_Y, zo), v3(40.0, GROUND_Y, zo), v3(40.0, GROUND_Y, zo + 45.0), v3(-40.0, GROUND_Y, zo + 45.0)], Vec3::Y, 4.0, [1.0; 3]);
    // наружная стена нашего дома (кирпич) с проёмами окон клетки
    let brick = [0.95, 0.62, 0.48];
    let top = FLOORS as f32 * H + 0.3;
    let fac = |b: &mut Building, x0: f32, x1: f32, y0: f32, y1: f32| {
        b.quad_w(Mat::Concrete, [v3(x0, y0, zo), v3(x1, y0, zo), v3(x1, y1, zo), v3(x0, y1, zo)], Vec3::Z, 1.0, brick);
    };
    fac(b, HOUSE_X0, WIN_X[0], GROUND_Y, top);
    fac(b, WIN_X[1], HOUSE_X1, GROUND_Y, top);
    let mut y = GROUND_Y;
    for n in 0..FLOORS - 1 {
        let f = mid_y(n);
        fac(b, WIN_X[0], WIN_X[1], y, f + WIN_Y[0]);
        y = f + WIN_Y[1];
    }
    fac(b, WIN_X[0], WIN_X[1], y, top);
    // дом напротив: панели 3.2 x 2.8, окно в каждой
    let zf = OPPOSITE_Z;
    let y_top = GROUND_Y + 0.6 + 5.0 * 2.8;
    b.quad_ws(Mat::Facade, [v3(40.0, GROUND_Y, zf), v3(-40.0, GROUND_Y, zf), v3(-40.0, y_top, zf), v3(40.0, y_top, zf)], -Vec3::Z, [3.2, 2.8], [-40.0, -y_top], [1.0; 3]);
    for j in 0..5 {
        for i in 0..25 {
            let x0 = -40.0 + i as f32 * 3.2;
            let y0 = GROUND_Y + 0.6 + j as f32 * 2.8;
            let (wx0, wx1, wy0, wy1) = (x0 + 0.85, x0 + 2.35, y0 + 0.8, y0 + 2.2);
            let z = zf - 0.02;
            let q = [v3(wx1, wy0, z), v3(wx0, wy0, z), v3(wx0, wy1, z), v3(wx1, wy1, z)];
            // отлив под окном и снег на нём, откосы — тень вокруг рамы
            b.cube(Mat::Metal, v3(wx0 - 0.05, wy0 - 0.04, zf - 0.12), v3(wx1 + 0.05, wy0, zf), 0.5, [0.4; 3]);
            b.cube(Mat::Snow, v3(wx0 - 0.03, wy0, zf - 0.11), v3(wx1 + 0.03, wy0 + 0.035, zf - 0.01), 4.0, [1.0; 3]);
            let r = hash01(i, j);
            // 23 часа: горит примерно каждое четвёртое; свет ламп накаливания
            // сквозь занавески — оранжевые, жёлтые, белые; редко — экран ТВ
            if r < 0.26 {
                let tints = [[1.0, 0.3, 0.07], [1.0, 0.5, 0.12], [1.0, 0.62, 0.3], [0.9, 0.22, 0.1], [0.45, 0.5, 0.75]];
                let pick = hash01(i + 7, j + 3);
                let t = tints[if pick > 0.94 { 4 } else { (pick * 4.0) as usize % 4 }];
                let k = (0.1 + 0.25 * hash01(i + 1, j + 9)) * 4.5;
                // свет из окон двух нижних этажей падает на снег под ними:
                // прожектор наружу, сила света = яркость окна * площадь
                // (Ламберт: I = L·A по нормали), конус ~75°
                // не больше 4: каждый источник — ещё теневой луч на каждый луч проб DDGI
                if j <= 1 && b.window_lamps.len() < 4 && (x0 + 1.6).abs() < 14.0 {
                    let lum = 10.0 * k * 0.6; // кд/м², ~0.6 — средняя пропускаемость шторы
                    let area = (wx1 - wx0) * (wy1 - wy0);
                    let c = v3((wx0 + wx1) * 0.5, (wy0 + wy1) * 0.5, zf - 0.3);
                    b.lamps.push(Lamp { pos: c, candela: lum * area, kelvin: 2400.0, kind: LampKind::Steady, bulb_r: 0.0, cone: Some((1.3, 0.6)), clear: false, range: 0.0 });
                    let n = b.lamps.len() - 1;
                    b.window_lamps.push(n);
                }
                // u — вариант занавески (четверть текстуры), v — сверху вниз
                let var = (hash01(i + 3, j + 11) * 4.0).floor() / 4.0;
                b.quad4(Mat::WinLit, q, [[var + 0.25, 1.0], [var, 1.0], [var, 0.0], [var + 0.25, 0.0]], -Vec3::Z, [t[0] * k, t[1] * k, t[2] * k]);
                // рама: импост и горизонталь форточки — тёмные на светлом
                let zz = z - 0.01;
                let xm = (wx0 + wx1) * 0.5;
                b.cube(Mat::Black, v3(xm - 0.03, wy0, zz - 0.02), v3(xm + 0.03, wy1, zz), 0.1, [0.03; 3]);
                b.cube(Mat::Black, v3(wx0, wy1 - 0.45, zz - 0.02), v3(xm, wy1 - 0.4, zz), 0.1, [0.03; 3]);
            } else {
                b.quad_w(Mat::WinDark, q, -Vec3::Z, 1.0, [1.0; 3]);
            }
        }
    }
    // балконы дома напротив: плита, ограждение из листа с полосой, снег
    // на плите; у некоторых — остеклённые рамы и бельё (верёвка)
    for j in 1..5 {
        for i in (1..25).step_by(3) {
            if hash01(i, j + 91) < 0.35 {
                continue;
            }
            let x0 = -40.0 + i as f32 * 3.2 + 0.5;
            let y0 = GROUND_Y + 0.6 + j as f32 * 2.8;
            let (bw, bd) = (2.2, 0.95);
            let z1 = OPPOSITE_Z;
            b.cube(Mat::Concrete, v3(x0, y0 - 0.12, z1 - bd), v3(x0 + bw, y0, z1), 1.0, [0.55; 3]);
            b.cube(Mat::Snow, v3(x0 + 0.05, y0, z1 - bd + 0.05), v3(x0 + bw - 0.05, y0 + 0.05, z1 - 0.05), 4.0, [1.0; 3]);
            let panel = if hash01(i, j + 93) < 0.5 { [0.35, 0.33, 0.3] } else { [0.5, 0.42, 0.3] };
            b.cube(Mat::Metal, v3(x0, y0, z1 - bd - 0.03), v3(x0 + bw, y0 + 1.0, z1 - bd), 0.5, panel);
            b.cube(Mat::Metal, v3(x0 - 0.03, y0, z1 - bd), v3(x0, y0 + 1.0, z1), 0.5, panel);
            b.cube(Mat::Metal, v3(x0 + bw, y0, z1 - bd), v3(x0 + bw + 0.03, y0 + 1.0, z1), 0.5, panel);
            b.cube(Mat::Snow, v3(x0 - 0.03, y0 + 1.0, z1 - bd - 0.03), v3(x0 + bw + 0.03, y0 + 1.04, z1 - bd + 0.02), 4.0, [1.0; 3]);
            if hash01(i, j + 95) < 0.4 {
                // застеклённый: рамы над ограждением
                for k in 0..5 {
                    let x = x0 + k as f32 * bw / 4.0;
                    b.cube(Mat::Wood, v3(x - 0.025, y0 + 1.0, z1 - bd - 0.03), v3(x + 0.025, y0 + 2.6, z1 - bd), 0.5, [0.6, 0.58, 0.52]);
                }
                b.quad_w(Mat::WinDark, [v3(x0, y0 + 1.0, z1 - bd - 0.015), v3(x0 + bw, y0 + 1.0, z1 - bd - 0.015), v3(x0 + bw, y0 + 2.6, z1 - bd - 0.015), v3(x0, y0 + 2.6, z1 - bd - 0.015)], -Vec3::Z, 1.0, [1.0; 3]);
            }
        }
    }
    // крыша дома напротив на фоне засвеченного неба: парапет, шахты
    // вентиляции, выходы на крышу и телевизионные антенны («ёлки»)
    let roof = GROUND_Y + 0.6 + 5.0 * 2.8;
    let zf = OPPOSITE_Z;
    b.cube(Mat::Concrete, v3(-40.0, roof, zf - 0.02), v3(40.0, roof + 0.45, zf + 0.25), 1.0, [0.55; 3]);
    b.cube(Mat::Snow, v3(-40.0, roof + 0.45, zf - 0.02), v3(40.0, roof + 0.5, zf + 0.25), 4.0, [1.0; 3]);
    for k in 0..7 {
        let x = -33.0 + k as f32 * 10.5 + hash01(k, 41) * 2.0;
        let (w, h) = (1.0 + hash01(k, 42), 1.1 + hash01(k, 43) * 0.6);
        b.cube(Mat::Concrete, v3(x - w / 2.0, roof, zf + 2.0), v3(x + w / 2.0, roof + h, zf + 2.0 + w * 0.7), 1.0, [0.5; 3]);
        b.cube(Mat::Snow, v3(x - w / 2.0, roof + h, zf + 2.0), v3(x + w / 2.0, roof + h + 0.08, zf + 2.0 + w * 0.7), 4.0, [1.0; 3]);
    }
    let dark = [0.08, 0.08, 0.09];
    for k in 0..9 {
        let x = -30.0 + k as f32 * 7.3 + hash01(k, 51) * 3.0;
        let z = zf + 3.0 + hash01(k, 52) * 6.0;
        let h = 2.2 + hash01(k, 53) * 1.6;
        // мачта на растяжках и поперечины разной длины
        b.tube(Mat::Metal, v3(x, roof, z), v3(x, roof + h, z), 0.02, 1.0, 6, dark);
        for r in 0..4 {
            let y = roof + h - 0.15 - r as f32 * 0.22;
            let half = 0.35 + r as f32 * 0.12 + hash01(k, 54 + r) * 0.1;
            let yaw = hash01(k, 60) * 3.0;
            let d = v3(yaw.cos(), 0.0, yaw.sin()) * half;
            b.tube(Mat::Metal, v3(x, y, z) - d, v3(x, y, z) + d, 0.008, 1.0, 4, dark);
        }
        for a in [0.4f32, 2.5, 4.6] {
            b.tube(Mat::Metal, v3(x, roof + h * 0.7, z), v3(x + a.cos() * 1.2, roof, z + a.sin() * 1.2), 0.003, 1.0, 3, dark);
        }
    }
    // сугробы вдоль протоптанной тропинки от нашего подъезда к дому
    // напротив: вытянуты вдоль тропинки и перекрываются — сплошной вал
    // счищенного снега, а не отдельные кучи
    for k in 0..14 {
        let side = if k % 2 == 0 { -1.0 } else { 1.0 };
        let z = D + 2.5 + (k / 2) as f32 * 3.2 + hash01(k, 71) * 1.2;
        let x = 0.9 + side * (1.7 + hash01(k, 72) * 0.4);
        let r = v3(1.0 + hash01(k, 73) * 0.5, 0.3 + hash01(k, 74) * 0.3, 2.0 + hash01(k, 75) * 1.0);
        b.mound(Mat::Snow, v3(x, GROUND_Y, z), r, [1.0; 3]);
    }
    // скамейка у подъезда напротив: бетонные ножки, рейки под снегом
    let (bx, bz) = (1.2, OPPOSITE_Z - 2.2);
    for dx in [-0.7f32, 0.7] {
        b.cube(Mat::Concrete, v3(bx + dx - 0.06, GROUND_Y, bz - 0.2), v3(bx + dx + 0.06, GROUND_Y + 0.42, bz + 0.2), 1.0, [0.55; 3]);
    }
    for k in 0..3 {
        let z = bz - 0.15 + k as f32 * 0.13;
        b.cube(Mat::Wood, v3(bx - 0.9, GROUND_Y + 0.42, z), v3(bx + 0.9, GROUND_Y + 0.46, z + 0.1), 0.5, [0.18, 0.1, 0.05]);
    }
    b.cube(Mat::Snow, v3(bx - 0.9, GROUND_Y + 0.46, bz - 0.17), v3(bx + 0.9, GROUND_Y + 0.52, bz + 0.25), 4.0, [1.0; 3]);
    // горка-«ракета»: крашеные трубы, корпус-каркас и конус
    let (rx, rz) = (-7.5, D + 15.0);
    let paint = [0.5, 0.08, 0.05];
    let h = 4.2;
    for k in 0..6 {
        let a = std::f32::consts::TAU * k as f32 / 6.0;
        let (ox, oz) = (a.cos() * 0.9, a.sin() * 0.9);
        b.tube(Mat::Metal, v3(rx + ox, GROUND_Y, rz + oz), v3(rx + ox * 0.9, GROUND_Y + h, rz + oz * 0.9), 0.025, 1.0, 6, paint);
        b.tube(Mat::Metal, v3(rx + ox * 0.9, GROUND_Y + h, rz + oz * 0.9), v3(rx, GROUND_Y + h + 1.3, rz), 0.02, 1.0, 6, paint);
    }
    for ring in 0..5 {
        let y = GROUND_Y + 0.4 + ring as f32 * 0.95;
        for k in 0..6 {
            let (a0, a1) = (std::f32::consts::TAU * k as f32 / 6.0, std::f32::consts::TAU * (k + 1) as f32 / 6.0);
            let rr = 0.9 - 0.1 * (y - GROUND_Y) / h;
            b.tube(Mat::Metal, v3(rx + a0.cos() * rr, y, rz + a0.sin() * rr), v3(rx + a1.cos() * rr, y, rz + a1.sin() * rr), 0.018, 1.0, 6, paint);
        }
    }
    // её скат: лист жести под снегом
    b.beam(Mat::Metal, v3(rx + 0.9, GROUND_Y + 1.35, rz), v3(rx + 3.4, GROUND_Y + 0.05, rz), Vec3::Z, 0.55, 0.03, [0.4, 0.4, 0.42]);
    b.beam(Mat::Snow, v3(rx + 0.9, GROUND_Y + 1.39, rz), v3(rx + 3.4, GROUND_Y + 0.09, rz), Vec3::Z, 0.5, 0.05, [1.0; 3]);
    // две машины под снегом у фонаря: кузов, стёкла под снегом, снежная шапка
    for (k, (cx, cz, yaw)) in [(2.2f32, D + 11.5f32, 0.08f32), (-4.6, D + 12.2, -0.05)].into_iter().enumerate() {
        car(b, cx, cz, GROUND_Y, yaw, if k == 0 { [0.35, 0.05, 0.04] } else { [0.12, 0.2, 0.32] });
    }
    // подъезд дома напротив: козырёк и лампочка над дверью
    let (px, pz) = (-1.6, OPPOSITE_Z);
    b.cube(Mat::Concrete, v3(px - 1.4, GROUND_Y + 2.6, pz - 1.3), v3(px + 1.4, GROUND_Y + 2.75, pz), 1.0, [0.6; 3]);
    b.cube(Mat::Snow, v3(px - 1.4, GROUND_Y + 2.75, pz - 1.3), v3(px + 1.4, GROUND_Y + 2.85, pz), 4.0, [1.0; 3]);
    b.cube(Mat::Wood, v3(px - 0.55, GROUND_Y, pz - 0.03), v3(px + 0.55, GROUND_Y + 2.1, pz), 0.5, [0.1, 0.07, 0.05]);
    b.lamps.push(Lamp { pos: v3(px, GROUND_Y + 2.45, pz - 0.25), candela: 45.0, kelvin: 2600.0, kind: LampKind::Steady, bulb_r: 0.03, cone: None, clear: false, range: 0.0 });
    // фонарь во дворе: столб и консоль
    let (lx, lz) = (5.5, D + 9.0);
    b.cube(Mat::Metal, v3(lx - 0.08, GROUND_Y, lz - 0.08), v3(lx + 0.08, 7.5, lz + 0.08), 0.5, [0.35, 0.36, 0.37]);
    b.tube(Mat::Metal, v3(lx, 7.4, lz), v3(lx - 1.2, 7.6, lz), 0.03, 1.0, 10, [0.35, 0.36, 0.37]);
    // корпус светильника ЖКУ: обтекаемый колпак (верхняя половина
    // эллипсоида) и под ним плоское стекло-рассеиватель
    b.ellipsoid(Mat::Metal, v3(lx - 1.3, 7.52, lz), v3(0.38, 0.14, 0.2), true, [0.3, 0.31, 0.32]);
    b.ellipsoid(Mat::Snow, v3(lx - 1.3, 7.58, lz), v3(0.3, 0.1, 0.15), true, [1.0; 3]);
    // натриевый светильник ЖКУ: отражатель с отсечкой — свет вниз и в
    // стороны до ~70° от вертикали, выше (на фасады, в окна) почти нет
    b.lamps.push(Lamp { pos: v3(lx - 1.3, 7.48, lz), candela: 4000.0, kelvin: 2000.0, kind: LampKind::Steady, bulb_r: 0.06, cone: Some((1.22, 0.85)), clear: false, range: 0.0 });
    // голые деревья двора (зима): берёзы и тополь
    for (k, (x, z, h, birch)) in [(-3.5f32, D + 6.5, 9.0f32, true), (9.5, D + 13.0, 12.0, false), (-9.0, D + 15.0, 10.0, true), (2.0, D + 19.0, 13.0, false)].into_iter().enumerate() {
        let bark = if birch { [0.72, 0.7, 0.66] } else { [0.16, 0.14, 0.12] };
        tree(b, v3(x, GROUND_Y, z), h, birch, k as u32 * 7 + 1, bark);
    }
}

/// Машина под снегом: кузов, стёкла, фары, номер, колёса, снежная шапка.
/// `yaw` — поворот оси машины от +Z, `gy` — уровень земли под ней.
fn car(b: &mut Building, cx: f32, cz: f32, gy: f32, yaw: f32, col: [f32; 3]) {
    let (l, w, hb) = (4.1, 1.62, 0.75);
    let (s, c) = (yaw.sin(), yaw.cos());
    let fwd = v3(s, 0.0, c);
    let rt = v3(c, 0.0, -s);
    let at = |a: f32, x: f32, y: f32| v3(cx, gy + y, cz) + fwd * a + rt * x;
    // нижняя часть кузова и салон — брусьями вдоль оси машины
    b.beam(Mat::Metal, at(-l / 2.0, 0.0, 0.25 + hb / 2.0 * 0.6), at(l / 2.0, 0.0, 0.25 + hb / 2.0 * 0.6), rt, w, hb * 0.6, col);
    b.beam(Mat::Metal, at(-0.9, 0.0, 0.25 + hb * 0.6 + 0.24), at(0.75, 0.0, 0.25 + hb * 0.6 + 0.24), rt, w * 0.86, 0.48, col);
    // стёкла салона (тёмные, блестят), фары и номер
    let cab_y = 0.25 + hb * 0.6 + 0.24;
    for sx in [-1.0f32, 1.0] {
        b.beam(Mat::WinDark, at(-0.75, sx * w * 0.432, cab_y + 0.04), at(0.6, sx * w * 0.432, cab_y + 0.04), fwd.cross(Vec3::Y), 0.01, 0.3, [1.0; 3]);
    }
    b.beam(Mat::WinDark, at(0.76, 0.0, cab_y + 0.02), at(0.78, 0.0, cab_y + 0.02), rt, w * 0.8, 0.34, [1.0; 3]);
    for x in [-0.55f32, 0.55] {
        b.beam(Mat::Wood, at(l / 2.0 - 0.01, x, 0.25 + hb * 0.35), at(l / 2.0 + 0.01, x, 0.25 + hb * 0.35), rt, 0.18, 0.12, [0.9, 0.88, 0.8]);
    }
    b.beam(Mat::Paper, at(l / 2.0 + 0.012, 0.0, 0.32), at(l / 2.0 + 0.014, 0.0, 0.32), rt, 0.52, 0.11, [1.0; 3]);
    // колёса
    for (a, x) in [(-1.25f32, -0.72f32), (-1.25, 0.72), (1.3, -0.72), (1.3, 0.72)] {
        b.tube(Mat::Black, at(a, x - 0.1, 0.3), at(a, x + 0.1, 0.3), 0.28, 1.0, 12, [0.03; 3]);
    }
    // снежная шапка: на крыше, капоте и багажнике
    let snow = [1.0; 3];
    b.beam(Mat::Snow, at(-0.95, 0.0, 0.25 + hb * 0.6 + 0.48 + 0.06), at(0.8, 0.0, 0.25 + hb * 0.6 + 0.48 + 0.06), rt, w * 0.9, 0.12, snow);
    b.beam(Mat::Snow, at(0.8, 0.0, 0.25 + hb * 0.6 + 0.05), at(l / 2.0, 0.0, 0.25 + hb * 0.6 + 0.04), rt, w * 1.0, 0.1, snow);
    b.beam(Mat::Snow, at(-l / 2.0, 0.0, 0.25 + hb * 0.6 + 0.04), at(-0.95, 0.0, 0.25 + hb * 0.6 + 0.05), rt, w * 1.0, 0.1, snow);
}

/// Голое дерево: ствол и ветви ветвятся рекурсивно, тоньше и короче с
/// каждым уровнем; у берёзы ветви тянутся вверх, тополь — колонной.
fn tree(b: &mut Building, base: Vec3, h: f32, birch: bool, seed: u32, bark: [f32; 3]) {
    let r0 = h * 0.018;
    branch(b, base, Vec3::Y, h * 0.42, r0, 0, seed, birch, bark);
}

#[allow(clippy::too_many_arguments)]
fn branch(b: &mut Building, from: Vec3, dir: Vec3, len: f32, r: f32, depth: u32, seed: u32, birch: bool, bark: [f32; 3]) {
    let to = from + dir * len;
    let side = if dir.cross(Vec3::X).length() > 0.1 { dir.cross(Vec3::X).normalize() } else { Vec3::Z };
    b.beam(Mat::Concrete, from, to, side, r * 2.0, r * 2.0, bark);
    if depth >= 4 || r < 0.006 {
        return;
    }
    let kids = if depth == 0 { 4 } else { 3 };
    for k in 0..kids {
        let s = seed.wrapping_mul(31).wrapping_add(k * 17 + depth * 101);
        let h = |n: u32| hash01(s as i32, n as i32);
        // точка отхождения — по длине ветви, ствол ветвится в верхней части
        let at = if depth == 0 { 0.45 + 0.55 * h(1) } else { 0.35 + 0.6 * h(1) };
        let p = from + dir * (len * at);
        let spread = if birch { 0.55 } else { 0.35 };
        let az = std::f32::consts::TAU * (k as f32 / kids as f32 + h(2) * 0.3);
        let tilt = spread * (0.6 + 0.8 * h(3));
        let a = dir.any_orthonormal_vector();
        let c = dir.cross(a);
        let mut nd = (dir * tilt.cos() + (a * az.cos() + c * az.sin()) * tilt.sin()).normalize();
        // вверх тянутся сильнее у тополя
        nd = (nd + Vec3::Y * if birch { 0.15 } else { 0.45 }).normalize();
        branch(b, p, nd, len * (0.55 + 0.2 * h(4)), r * 0.58, depth + 1, s, birch, bark);
    }
    // ствол продолжается вверх
    if depth == 0 {
        branch(b, to, (dir + Vec3::X * 0.05).normalize(), len * 0.8, r * 0.7, depth + 1, seed + 999, birch, bark);
    }
}

fn hash01(i: i32, j: i32) -> f32 {
    let mut h = (i as u32).wrapping_mul(73856093) ^ (j as u32).wrapping_mul(19349663);
    h = (h ^ (h >> 13)).wrapping_mul(1274126177);
    h ^= h >> 16;
    h as f32 / u32::MAX as f32
}

fn smooth01(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Плавный value noise 0..1 на решётке с шагом 1.
fn vnoise(x: f32, y: f32, seed: i32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (tx, ty) = (smooth01(x - x0), smooth01(y - y0));
    let (xi, yi) = (x0 as i32, y0 as i32);
    let h = |i: i32, j: i32| hash01(i.wrapping_add(seed.wrapping_mul(7919)), j);
    let top = h(xi, yi) + (h(xi + 1, yi) - h(xi, yi)) * tx;
    let bot = h(xi, yi + 1) + (h(xi + 1, yi + 1) - h(xi, yi + 1)) * tx;
    top + (bot - top) * ty
}

fn lamps(b: &mut Building) {
    for n in 0..FLOORS {
        let ceil = landing_y(n) + H - SLAB;
        let (x, z) = (1.25, 0.8);
        let drop = 0.32;
        // открытая проводка: от щитка вверх по стене и по потолку к лампе
        let cab = [0.05, 0.05, 0.05];
        let y = landing_y(n);
        let px = 2.15;
        if n >= 1 {
            for dx in [0.0f32, 0.012, 0.024] {
                b.cube(Mat::Black, v3(px + dx - 0.004, y + 2.05, 0.008), v3(px + dx + 0.004, ceil, 0.016), 0.1, cab);
            }
        }
        b.cube(Mat::Black, v3(x - 0.004, ceil - 0.008, 0.0), v3(x + 0.004, ceil, z), 0.1, cab);
        b.cube(Mat::Black, v3(x, ceil - 0.008, 0.004), v3(px, ceil, 0.012), 0.1, cab);
        // распаечная коробка на потолке
        b.cube(Mat::Metal, v3(x - 0.04, ceil - 0.03, 0.0), v3(x + 0.04, ceil, 0.06), 0.5, [0.75, 0.73, 0.68]);
        // выключатель у двери и провод от него вверх
        let sx = if n == 0 { 0.55 } else { 0.62 };
        b.cube(Mat::Metal, v3(sx - 0.035, y + 1.4, 0.0), v3(sx + 0.035, y + 1.48, 0.02), 0.5, [0.8, 0.78, 0.72]);
        b.cube(Mat::Black, v3(sx - 0.012, y + 1.43, 0.02), v3(sx + 0.012, y + 1.45, 0.028), 0.1, [0.4; 3]);
        b.cube(Mat::Black, v3(sx - 0.003, y + 1.48, 0.006), v3(sx + 0.003, ceil, 0.012), 0.1, cab);
        // паутина в верхнем углу у передней стены (там, куда не достаёт веник)
        if n != 1 {
            let (cx, cz) = (if n % 2 == 0 { 0.0 } else { W }, 0.0);
            let sgn = if n % 2 == 0 { 1.0 } else { -1.0 };
            let s = 0.28;
            let p0 = v3(cx, ceil, cz);
            b.quad4(Mat::Web, [p0, p0 + v3(sgn * s, 0.0, 0.0), p0 + v3(0.0, 0.0, s) + v3(sgn * s * 0.6, -s * 0.5, 0.0), p0 + v3(0.0, -s, 0.0) + v3(0.0, 0.0, s * 0.3)], [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], v3(sgn, -1.0, 1.0).normalize(), [0.85, 0.85, 0.85]);
        }
        // трещина в побелке по стыку плит перекрытия (поперёк потолка
        // площадки) и по углу стены — у каждого этажа своя часть текстуры
        let cy = ceil - 0.0025;
        let cz = 0.35 + (n as f32 * 0.29).fract() * 0.6;
        let v0 = (n % 4) as f32 * 0.25;
        b.quad4(Mat::Crack, [v3(0.0, cy, cz - 0.06), v3(W, cy, cz - 0.06), v3(W, cy, cz + 0.06), v3(0.0, cy, cz + 0.06)], [[0.0, v0], [1.0, v0], [1.0, v0 + 0.25], [0.0, v0 + 0.25]], -Vec3::Y, [1.0; 3]);
        // копоть на побелке над лампой: десятилетия горячего воздуха от колбы
        let sr = 0.7;
        let sy = ceil - 0.002;
        b.quad4(Mat::Soot, [v3(x - sr, sy, z - sr), v3(x + sr, sy, z - sr), v3(x + sr, sy, z + sr), v3(x - sr, sy, z + sr)], [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]], -Vec3::Y, [1.0; 3]);
        // провод и патрон
        b.cube(Mat::Black, v3(x - 0.004, ceil - drop + 0.06, z - 0.004), v3(x + 0.004, ceil, z + 0.004), 0.1, [0.15; 3]);
        b.cube(Mat::Black, v3(x - 0.02, ceil - drop + 0.02, z - 0.02), v3(x + 0.02, ceil - drop + 0.07, z + 0.02), 0.1, [0.12; 3]);
        let kind = match n {
            2 => LampKind::Flicker,
            3 => LampKind::Dead,
            _ => LampKind::Steady,
        };
        // 60 Вт лампа накаливания: ~710 лм -> 56 кд; на пятом 40 Вт
        let candela = if n == 4 { 33.0 } else { 56.0 };
        let lp = v3(x, ceil - drop - 0.03, z);
        // цоколь E27 (латунь) от горлышка колбы до патрона, с витками резьбы
        b.cyl(Mat::Brass, lp + v3(0.0, 0.024, 0.0), 0.026, 0.0135, 16, [1.0; 3]);
        for k in 0..3 {
            b.cyl(Mat::Brass, lp + v3(0.0, 0.029 + k as f32 * 0.007, 0.0), 0.0025, 0.0145, 16, [1.0; 3]);
        }
        b.lamps.push(Lamp { pos: lp, candela, kelvin: 2700.0, kind, bulb_r: 0.01, cone: None, clear: true, range: 0.0 });
    }
}

// ---------------------------------------------------------------------------
// ДОБАВЛЕНО (выход на улицу): двор перед домом — по панораме самарского
// двора хрущёвки: длинная пятиэтажка на 4 подъезда, светлая штукатурка и
// красно-коричневый цоколь, жёлтая газовая труба вдоль фасада, решётки на
// окнах первого этажа, крыльца с козырьками; узкий проезд с высокими
// бордюрами, палисадники за низкой оградой, машины на снегу через проезд,
// кирпичная ограда с белёными арками, за ней гаражи, контейнерная площадка.
// Зима, ночь — как и всё остальное.
// ---------------------------------------------------------------------------

/// Проём входной двери (x) и петля полотна: дверь открывается наружу.
pub const DOOR_X0: f32 = 0.75;
pub const DOOR_X1: f32 = 1.75;
pub const DOOR_HINGE: Vec3 = Vec3::new(DOOR_X0, 0.0, 0.0);
/// Наружная плоскость фасада со стороны входа.
pub const FRONT_Z: f32 = -WALL_T;
/// Подъезды: центры дверей по x (наш — второй, x = W / 2).
pub const ENTRANCES: [f32; 4] = [W / 2.0 - SECTION, W / 2.0, W / 2.0 + SECTION, W / 2.0 + 2.0 * SECTION];
/// Наш подъезд — номер 2.
pub const OUR_ENTRANCE: usize = 1;
const SECTION: f32 = 13.5;
pub const HOUSE_X0: f32 = W / 2.0 - 1.5 * SECTION - 0.5;
pub const HOUSE_X1: f32 = W / 2.0 + 2.5 * SECTION + 0.5;
/// Проезд вдоль дома: асфальт ниже газона на высоту бордюра.
const ROAD_Z: [f32; 2] = [-8.6, -3.6];
const ROAD_Y: f32 = GROUND_Y - 0.12;
/// Низкая ограда палисадников и проходы в ней — у каждого подъезда.
const GARDEN_Z: f32 = -3.35;
const PATH_HALF: f32 = 0.85;
/// Кирпичная ограда за стоянкой — дальше не пройти.
const FENCE_Z: f32 = -11.8;
/// Контейнерная площадка.
pub const TRASH: [f32; 4] = [20.0, -11.4, 25.0, -9.6];
const CARS: [(f32, f32, f32, [f32; 3]); 4] = [
    (-7.5, -10.0, 0.04, [0.55, 0.55, 0.52]),
    (-4.4, -10.2, -0.03, [0.08, 0.09, 0.1]),
    (5.6, -10.1, 0.06, [0.38, 0.1, 0.06]),
    (12.3, -9.9, -0.05, [0.2, 0.26, 0.2]),
];
const LAMP_POST: (f32, f32) = (8.6, -9.2);
const TREES: [(f32, f32, f32); 6] = [(-9.0, -2.2, 9.5), (-3.2, -1.8, 11.0), (5.0, -2.0, 10.0), (10.5, -2.4, 8.5), (19.5, -1.9, 10.5), (-1.0, -9.6, 12.0)];

/// Высота земли снаружи под точкой (x, z): крыльцо, ступень, порог,
/// газон, бордюр, проезд.
pub fn outside_floor(x: f32, z: f32) -> f32 {
    if z >= FRONT_Z && z <= 0.0 {
        return 0.0; // порог в проёме
    }
    for &c in &ENTRANCES {
        if (x - c).abs() <= 1.1 {
            if z >= -1.25 && z < FRONT_Z {
                return 0.0; // площадка крыльца
            }
            if z >= -1.55 && z < -1.25 {
                return -0.2; // ступень
            }
        }
    }
    if z > ROAD_Z[0] && z < ROAD_Z[1] {
        ROAD_Y
    } else {
        GROUND_Y
    }
}

/// Препятствия снаружи (x0, z0, x1, z1): машины, фонарь, стволы, ограда
/// палисадников (кроме проходов), контейнерная площадка, дома.
pub fn outside_obstacles() -> Vec<[f32; 4]> {
    let mut o = Vec::new();
    for &(cx, cz, _, _) in &CARS {
        o.push([cx - 0.85, cz - 2.1, cx + 0.85, cz + 2.1]);
    }
    o.push([LAMP_POST.0 - 0.2, LAMP_POST.1 - 0.2, LAMP_POST.0 + 0.2, LAMP_POST.1 + 0.2]);
    for &(x, z, _) in &TREES {
        o.push([x - 0.25, z - 0.25, x + 0.25, z + 0.25]);
    }
    let mut x = HOUSE_X0;
    for &c in &ENTRANCES {
        o.push([x, GARDEN_Z - 0.08, c - PATH_HALF, GARDEN_Z + 0.08]);
        x = c + PATH_HALF;
    }
    o.push([x, GARDEN_Z - 0.08, HOUSE_X1, GARDEN_Z + 0.08]);
    // площадка: задняя и боковые стенки, контейнеры внутри
    o.push([TRASH[0], TRASH[1], TRASH[2], TRASH[1] + 0.9]);
    o.push([TRASH[0], TRASH[1], TRASH[0] + 0.15, TRASH[3]]);
    o.push([TRASH[2] - 0.15, TRASH[1], TRASH[2], TRASH[3]]);
    // торцы дома: за угол во двор за домом не пройти (там окна клетки)
    o.push([HOUSE_X0 - 0.4, FRONT_Z - 0.4, HOUSE_X0, 0.0]);
    o.push([HOUSE_X1, FRONT_Z - 0.4, HOUSE_X1 + 0.4, 0.0]);
    o
}

/// Границы улицы для ходьбы (x0, z0, x1, z1).
pub const OUTSIDE_BOUNDS: [f32; 4] = [HOUSE_X0 - 6.0, FENCE_Z + 0.4, HOUSE_X1 + 6.0, FRONT_Z];

fn front_yard(b: &mut Building) {
    let zf = FRONT_Z;
    let top = FLOORS as f32 * H + 0.3;
    // ИЗМЕНЕНО (текстуры двора): цвет штукатурки и цоколя — в самих
    // текстурах (tex::plaster, tex::plinth), тон вершин нейтральный
    let plaster = [1.0; 3];
    let base_col = [1.0; 3];
    let (x0h, x1h) = (HOUSE_X0, HOUSE_X1);
    let (ox0, ox1) = (DOOR_X0 - 0.07, DOOR_X1 + 0.07);

    // ---- фасад нашего дома: штукатурка с проёмом нашей двери ----
    let fac = |b: &mut Building, xa: f32, xb: f32, ya: f32, yb: f32| {
        b.quad_ws(Mat::Plaster, [v3(xb, ya, zf), v3(xa, ya, zf), v3(xa, yb, zf), v3(xb, yb, zf)], -Vec3::Z, [4.8, 4.8], [0.0, 0.0], plaster);
    };
    fac(b, x0h, ox0, GROUND_Y, top);
    fac(b, ox1, x1h, GROUND_Y, top);
    fac(b, ox0, ox1, 2.17, top);
    // откосы проёма, перемычка снизу и порог
    b.quad_w(Mat::Concrete, [v3(ox0, 0.0, zf), v3(ox0, 0.0, 0.0), v3(ox0, 2.17, 0.0), v3(ox0, 2.17, zf)], Vec3::X, 1.0, [0.62; 3]);
    b.quad_w(Mat::Concrete, [v3(ox1, 0.0, 0.0), v3(ox1, 0.0, zf), v3(ox1, 2.17, zf), v3(ox1, 2.17, 0.0)], -Vec3::X, 1.0, [0.62; 3]);
    b.quad_w(Mat::Concrete, [v3(ox0, 2.17, zf), v3(ox0, 2.17, 0.0), v3(ox1, 2.17, 0.0), v3(ox1, 2.17, zf)], -Vec3::Y, 1.0, [0.55; 3]);
    b.quad_w(Mat::Concrete, [v3(ox0, 0.0, 0.0), v3(ox0, 0.0, zf), v3(ox1, 0.0, zf), v3(ox1, 0.0, 0.0)], Vec3::Y, 1.0, [0.5; 3]);
    // торцы и крыша (видны из двора, если поднять голову)
    b.quad_ws(Mat::Plaster, [v3(x0h, GROUND_Y, zf), v3(x0h, GROUND_Y, D + WALL_T), v3(x0h, top, D + WALL_T), v3(x0h, top, zf)], -Vec3::X, [4.8, 4.8], [0.0, 0.0], plaster);
    b.quad_ws(Mat::Plaster, [v3(x1h, GROUND_Y, D + WALL_T), v3(x1h, GROUND_Y, zf), v3(x1h, top, zf), v3(x1h, top, D + WALL_T)], Vec3::X, [4.8, 4.8], [0.0, 0.0], plaster);
    b.quad_w(Mat::Snow, [v3(x0h, top, zf), v3(x1h, top, zf), v3(x1h, top, D + WALL_T), v3(x0h, top, D + WALL_T)], Vec3::Y, 4.0, [1.0; 3]);
    b.cube(Mat::Concrete, v3(x0h, top - 0.05, zf - 0.12), v3(x1h, top + 0.25, zf + 0.1), 1.0, [0.6; 3]);
    b.cube(Mat::Snow, v3(x0h, top + 0.25, zf - 0.12), v3(x1h, top + 0.33, zf + 0.1), 4.0, [1.0; 3]);
    // цоколь: красно-коричневая полоса, у крылец — за ними (не видно)
    let mut x = x0h;
    for &c in &ENTRANCES {
        b.cube(Mat::Plinth, v3(x, GROUND_Y, zf - 0.04), v3(c - 1.1, 0.42, zf), 1.0, base_col);
        x = c + 1.1;
    }
    b.cube(Mat::Plinth, v3(x, GROUND_Y, zf - 0.04), v3(x1h, 0.42, zf), 1.0, base_col);

    // ---- окна квартир: по 4 на этаж в секции, клетка — глухая ----
    for (si, &c) in ENTRANCES.iter().enumerate() {
        for f in 0..FLOORS {
            for (wi, dx) in [-5.0f32, -2.6, 2.6, 5.0].into_iter().enumerate() {
                let (i, j) = ((si * 4 + wi) as i32, f as i32);
                let wx = c + dx;
                let (wx0, wx1) = (wx - 0.7, wx + 0.7);
                let (wy0, wy1) = (f as f32 * H + 0.85, f as f32 * H + 2.25);
                let z = zf - 0.02;
                let q = [v3(wx1, wy0, z), v3(wx0, wy0, z), v3(wx0, wy1, z), v3(wx1, wy1, z)];
                // отлив и снег на нём, рама — белые (пластик) или тёмные (дерево)
                b.cube(Mat::Metal, v3(wx0 - 0.05, wy0 - 0.04, zf - 0.12), v3(wx1 + 0.05, wy0, zf), 0.5, [0.45; 3]);
                b.cube(Mat::Snow, v3(wx0 - 0.03, wy0, zf - 0.11), v3(wx1 + 0.03, wy0 + 0.03, zf - 0.01), 4.0, [1.0; 3]);
                let r = hash01(i + 211, j + 17);
                // 23 часа: горит примерно каждое четвёртое
                if r < 0.27 {
                    let tints = [[1.0, 0.3, 0.07], [1.0, 0.5, 0.12], [1.0, 0.62, 0.3], [0.9, 0.22, 0.1], [0.45, 0.5, 0.75]];
                    let pick = hash01(i + 9, j + 5);
                    let t = tints[if pick > 0.94 { 4 } else { (pick * 4.0) as usize % 4 }];
                    let k = (0.1 + 0.25 * hash01(i + 3, j + 13)) * 4.5;
                    let var = (hash01(i + 5, j + 19) * 4.0).floor() / 4.0;
                    b.quad4(Mat::WinLit, q, [[var + 0.25, 1.0], [var, 1.0], [var, 0.0], [var + 0.25, 0.0]], -Vec3::Z, [t[0] * k, t[1] * k, t[2] * k]);
                } else {
                    b.quad_w(Mat::WinDark, q, -Vec3::Z, 1.0, [1.0; 3]);
                }
                let frame = if hash01(i + 41, j + 3) < 0.6 { [0.85; 3] } else { [0.22, 0.14, 0.09] };
                let zz = z - 0.01;
                let xm = (wx0 + wx1) * 0.5;
                b.cube(Mat::Wood, v3(xm - 0.03, wy0, zz - 0.03), v3(xm + 0.03, wy1, zz), 0.5, frame);
                b.cube(Mat::Wood, v3(wx0, wy1 - 0.45, zz - 0.03), v3(xm, wy1 - 0.4, zz), 0.5, frame);
                for (ya, yb) in [(wy0, wy0 + 0.05), (wy1 - 0.05, wy1)] {
                    b.cube(Mat::Wood, v3(wx0, ya, zz - 0.03), v3(wx1, yb, zz), 0.5, frame);
                }
                // решётки на окнах первого этажа
                if f == 0 {
                    let mut bx = wx0 + 0.06;
                    while bx < wx1 - 0.03 {
                        b.cube(Mat::Metal, v3(bx - 0.008, wy0 - 0.02, zf - 0.1), v3(bx + 0.008, wy1 + 0.04, zf - 0.084), 0.5, [0.85; 3]);
                        bx += 0.12;
                    }
                    for yy in [wy0 + 0.35, wy1 - 0.3] {
                        b.cube(Mat::Metal, v3(wx0 - 0.02, yy - 0.01, zf - 0.11), v3(wx1 + 0.02, yy + 0.01, zf - 0.084), 0.5, [0.85; 3]);
                    }
                }
                // балконы — у крайних окон секции, со 2-го этажа, не у всех
                if f >= 1 && wi % 3 == 0 && hash01(i + 77, j) < 0.7 {
                    let y = f as f32 * H;
                    let (bx0, bx1, bz) = (wx - 1.0, wx + 1.0, zf - 0.95);
                    b.cube(Mat::Concrete, v3(bx0, y - 0.12, bz), v3(bx1, y, zf), 1.0, [0.6; 3]);
                    let glazed = hash01(i + 3, j + 7) < 0.55;
                    if !glazed {
                        // открытый балкон: снег на плите внутри ограждения
                        b.cube(Mat::Snow, v3(bx0 + 0.03, y, bz + 0.03), v3(bx1 - 0.03, y + 0.05, zf), 4.0, [1.0; 3]);
                    }
                    let rail = if hash01(i, j + 5) < 0.5 { [0.55, 0.62, 0.55] } else { [0.7, 0.7, 0.68] };
                    b.cube(Mat::Metal, v3(bx0, y, bz), v3(bx1, y + 1.0, bz + 0.02), 0.5, rail);
                    b.cube(Mat::Metal, v3(bx0, y, bz), v3(bx0 + 0.02, y + 1.0, zf), 0.5, rail);
                    b.cube(Mat::Metal, v3(bx1 - 0.02, y, bz), v3(bx1, y + 1.0, zf), 0.5, rail);
                    b.cube(Mat::Snow, v3(bx0, y + 1.0, bz - 0.01), v3(bx1, y + 1.04, bz + 0.05), 4.0, [1.0; 3]);
                    if glazed {
                        // застеклён: рамы над ограждением до плиты выше
                        b.quad_w(Mat::WinDark, [v3(bx1, y + 1.0, bz), v3(bx0, y + 1.0, bz), v3(bx0, y + H - 0.15, bz), v3(bx1, y + H - 0.15, bz)], -Vec3::Z, 1.0, [1.0; 3]);
                        let mut fx = bx0;
                        while fx <= bx1 + 0.01 {
                            b.cube(Mat::Wood, v3(fx - 0.025, y + 1.0, bz - 0.03), v3(fx + 0.025, y + H - 0.15, bz), 0.5, [0.8; 3]);
                            fx += 0.5;
                        }
                        b.cube(Mat::Concrete, v3(bx0, y + H - 0.15, bz), v3(bx1, y + H - 0.12, zf), 1.0, [0.6; 3]);
                    }
                }
            }
        }
    }

    // ---- подъезды: крыльцо, козырёк, лампа, номер, дверь с домофоном ----
    for (k, &c) in ENTRANCES.iter().enumerate() {
        b.cube(Mat::Concrete, v3(c - 1.1, GROUND_Y, -1.25), v3(c + 1.1, 0.0, zf), 1.0, [0.55; 3]);
        b.cube(Mat::Concrete, v3(c - 1.1, GROUND_Y, -1.55), v3(c + 1.1, -0.2, -1.25), 1.0, [0.52; 3]);
        // утоптанный снег на краях крыльца
        b.cube(Mat::Snow, v3(c - 1.1, 0.0, -1.25), v3(c - 0.75, 0.03, zf), 4.0, [0.8; 3]);
        b.cube(Mat::Snow, v3(c + 0.75, 0.0, -1.25), v3(c + 1.1, 0.03, zf), 4.0, [0.8; 3]);
        // козырёк: плита на двух консолях, снег на нём
        b.cube(Mat::Concrete, v3(c - 1.3, 2.55, -1.6), v3(c + 1.3, 2.68, zf), 1.0, [0.6; 3]);
        b.cube(Mat::Snow, v3(c - 1.3, 2.68, -1.6), v3(c + 1.3, 2.8, zf), 4.0, [1.0; 3]);
        for sx in [-1.15f32, 1.15] {
            b.beam(Mat::Metal, v3(c + sx, 2.0, zf), v3(c + sx, 2.55, -1.3), Vec3::X, 0.05, 0.05, [0.3; 3]);
        }
        // лампа под козырьком: патрон на кронштейне
        let lamp = v3(c, 2.38, zf - 0.3);
        b.beam(Mat::Black, v3(c, 2.55, zf - 0.3), v3(c, 2.43, zf - 0.3), Vec3::X, 0.04, 0.04, [0.05; 3]);
        b.lamps.push(Lamp { pos: lamp, candela: 38.0, kelvin: 2650.0, kind: LampKind::Steady, bulb_r: 0.03, cone: None, clear: false, range: if k == OUR_ENTRANCE { 14.0 } else { 10.0 } });
        // номер подъезда над дверью: синяя табличка с белой цифрой
        b.cube(Mat::Metal, v3(c - 0.2, 2.22, zf - 0.02), v3(c + 0.2, 2.48, zf), 0.5, [0.12, 0.25, 0.5]);
        let d = (k + 1) as f32;
        let (hw, y0, y1, zd) = (0.08, 2.25, 2.45, zf - 0.025);
        b.quad4(Mat::Digits, [v3(c + hw, y0, zd), v3(c - hw, y0, zd), v3(c - hw, y1, zd), v3(c + hw, y1, zd)], [[d / 10.0, 1.0], [(d + 1.0) / 10.0, 1.0], [(d + 1.0) / 10.0, 0.0], [d / 10.0, 0.0]], -Vec3::Z, [0.95; 3]);
        // домофон справа от двери: корпус, решётка динамика, кнопки
        let ix = c + 0.82;
        b.cube(Mat::Metal, v3(ix - 0.09, 1.2, zf - 0.035), v3(ix + 0.09, 1.55, zf), 0.5, [0.2; 3]);
        b.cube(Mat::Black, v3(ix - 0.06, 1.44, zf - 0.04), v3(ix + 0.06, 1.51, zf - 0.035), 0.1, [0.02; 3]);
        for r in 0..3 {
            for cc in 0..3 {
                let (bx, by) = (ix - 0.05 + cc as f32 * 0.05, 1.25 + r as f32 * 0.05);
                b.cube(Mat::Brass, v3(bx - 0.012, by - 0.012, zf - 0.045), v3(bx + 0.012, by + 0.012, zf - 0.035), 0.1, [0.55; 3]);
            }
        }
        // чужие подъезды: металлическая дверь заподлицо с фасадом, ручка
        if k != OUR_ENTRANCE {
            b.cube(Mat::Metal, v3(c - 0.5, 0.0, zf - 0.06), v3(c + 0.5, 2.1, zf), 0.5, [0.36, 0.37, 0.38]);
            b.cube(Mat::Black, v3(c - 0.55, 0.0, zf - 0.04), v3(c + 0.55, 2.15, zf), 0.1, [0.02; 3]);
            b.tube(Mat::Metal, v3(c + 0.38, 0.95, zf - 0.12), v3(c + 0.38, 1.3, zf - 0.12), 0.012, 1.0, 10, [0.6; 3]);
        }
        // дорожка от крыльца к проезду — расчищена, по краям снег
        b.quad_w(Mat::Concrete, [v3(c - PATH_HALF, GROUND_Y + 0.004, -1.55), v3(c + PATH_HALF, GROUND_Y + 0.004, -1.55), v3(c + PATH_HALF, GROUND_Y + 0.004, ROAD_Z[1]), v3(c - PATH_HALF, GROUND_Y + 0.004, ROAD_Z[1])], Vec3::Y, 0.5, [0.42, 0.42, 0.44]);
        for sx in [-1.0f32, 1.0] {
            let mut z = -1.8;
            while z > ROAD_Z[1] + 0.2 {
                b.mound(Mat::Snow, v3(c + sx * (PATH_HALF + 0.15), GROUND_Y, z), v3(0.25, 0.18 + 0.08 * hash01((c * 3.0) as i32, (z * 7.0) as i32), 0.35), [1.0; 3]);
                z -= 0.5;
            }
        }
    }

    // ---- газовая труба: вдоль фасада над окнами первого этажа, отводы вниз ----
    let yellow = [0.62, 0.42, 0.02];
    let (py, pz) = (2.45, zf - 0.12);
    b.tube(Mat::Metal, v3(x0h + 0.3, py, pz), v3(x1h - 0.3, py, pz), 0.032, 1.0, 10, yellow);
    let mut bx = x0h + 1.0;
    while bx < x1h - 0.5 {
        b.cube(Mat::Metal, v3(bx - 0.02, py - 0.06, pz), v3(bx + 0.02, py - 0.03, zf), 0.5, [0.3; 3]);
        bx += 2.0;
    }
    for &c in &ENTRANCES {
        let dx = c - 2.1;
        b.tube(Mat::Metal, v3(dx, py, pz), v3(dx, 0.55, pz), 0.032, 1.0, 10, yellow);
        b.tube(Mat::Metal, v3(dx, 0.55, pz), v3(dx, 0.55, zf), 0.032, 1.0, 10, yellow);
        // кран с красной рукояткой
        b.cube(Mat::Metal, v3(dx - 0.05, 1.0, pz - 0.05), v3(dx + 0.05, 1.08, pz + 0.05), 0.5, yellow);
        b.cube(Mat::Metal, v3(dx - 0.01, 1.03, pz - 0.16), v3(dx + 0.01, 1.05, pz - 0.05), 0.5, [0.6, 0.06, 0.04]);
    }

    // ---- земля: газон (палисадники), проезд ниже на бордюр, стоянка ----
    b.quad_w(Mat::Snow, [v3(-45.0, GROUND_Y, ROAD_Z[1]), v3(60.0, GROUND_Y, ROAD_Z[1]), v3(60.0, GROUND_Y, zf), v3(-45.0, GROUND_Y, zf)], Vec3::Y, 4.0, [1.0; 3]);
    b.quad_w(Mat::Snow, [v3(-45.0, GROUND_Y, -45.0), v3(60.0, GROUND_Y, -45.0), v3(60.0, GROUND_Y, ROAD_Z[0]), v3(-45.0, GROUND_Y, ROAD_Z[0])], Vec3::Y, 4.0, [1.0; 3]);
    // проезд: укатанный серый снег, колеи темнее — укатанные до льда
    // полосы неровной ширины, местами припорошены (кусками по 1.5 м)
    b.quad_w(Mat::Snow, [v3(-45.0, ROAD_Y, ROAD_Z[0]), v3(60.0, ROAD_Y, ROAD_Z[0]), v3(60.0, ROAD_Y, ROAD_Z[1]), v3(-45.0, ROAD_Y, ROAD_Z[1])], Vec3::Y, 4.0, [0.6, 0.6, 0.63]);
    for (t, zc) in [-4.92f32, -6.32].into_iter().enumerate() {
        let mut x = -45.0f32;
        while x < 60.0 {
            let n = |k: i32| hash01((x * 0.7) as i32 + k, t as i32 * 13 + 5);
            let (w0, w1) = (0.16 + 0.1 * n(0), 0.16 + 0.1 * n(1));
            let (o0, o1) = (0.06 * (n(2) - 0.5), 0.06 * (n(3) - 0.5));
            let k = 0.32 + 0.12 * n(4);
            let y = ROAD_Y + 0.003;
            b.quad_w(Mat::Concrete, [v3(x, y, zc + o0 - w0), v3(x + 1.5, y, zc + o1 - w1), v3(x + 1.5, y, zc + o1 + w1), v3(x, y, zc + o0 + w0)], Vec3::Y, 1.0, [k, k, k * 1.04]);
            x += 1.5;
        }
    }
    // бордюры: высокие (15 см), со снегом сверху
    for (za, zb) in [(ROAD_Z[1], ROAD_Z[1] + 0.15), (ROAD_Z[0] - 0.15, ROAD_Z[0])] {
        b.cube(Mat::Concrete, v3(-45.0, ROAD_Y, za), v3(60.0, GROUND_Y + 0.03, zb), 1.0, [0.62; 3]);
    }
    // валы счищенного снега вдоль проезда — кроме проходов к подъездам
    let mut z_side = [ROAD_Z[1] + 0.45, ROAD_Z[0] - 0.5];
    for (side, z) in z_side.iter_mut().enumerate() {
        let mut x = -30.0;
        while x < 50.0 {
            let near_path = side == 0 && ENTRANCES.iter().any(|&c| (x - c).abs() < PATH_HALF + 0.5);
            let near_car = side == 1 && CARS.iter().any(|&(cx, _, _, _)| (x - cx).abs() < 1.2);
            // к контейнерам расчищен проход
            let near_trash = side == 1 && x > TRASH[0] - 1.0 && x < TRASH[2] + 1.0;
            if !near_path && !near_car && !near_trash {
                let h = 0.25 + 0.2 * hash01((x * 2.0) as i32, side as i32 + 3);
                b.mound(Mat::Snow, v3(x, GROUND_Y, *z), v3(0.9, h, 0.45), [1.0; 3]);
            }
            x += 0.8;
        }
    }

    // ---- палисадники: низкая ограда из труб, кусты, деревья ----
    let green = [0.12, 0.3, 0.17];
    let fence = |b: &mut Building, xa: f32, xb: f32| {
        b.tube(Mat::Metal, v3(xa, GROUND_Y + 0.55, GARDEN_Z), v3(xb, GROUND_Y + 0.55, GARDEN_Z), 0.02, 1.0, 8, green);
        b.tube(Mat::Metal, v3(xa, GROUND_Y + 0.25, GARDEN_Z), v3(xb, GROUND_Y + 0.25, GARDEN_Z), 0.015, 1.0, 8, green);
        let mut x = xa;
        while x <= xb {
            b.cube(Mat::Metal, v3(x - 0.02, GROUND_Y, GARDEN_Z - 0.02), v3(x + 0.02, GROUND_Y + 0.58, GARDEN_Z + 0.02), 0.5, green);
            x += 1.5;
        }
    };
    let mut x = x0h;
    for &c in &ENTRANCES {
        fence(b, x, c - PATH_HALF);
        x = c + PATH_HALF;
    }
    fence(b, x, x1h);
    // кусты под снегом
    let mut x = x0h + 1.0;
    while x < x1h - 1.0 {
        if !ENTRANCES.iter().any(|&c| (x - c).abs() < 2.0) && hash01((x * 3.0) as i32, 41) < 0.6 {
            let zc = -1.4 - 1.2 * hash01((x * 5.0) as i32, 43);
            b.mound(Mat::Snow, v3(x, GROUND_Y, zc), v3(0.6 + 0.4 * hash01((x * 2.0) as i32, 47), 0.45 + 0.3 * hash01((x * 7.0) as i32, 49), 0.55), [1.0; 3]);
            branch(b, v3(x, GROUND_Y + 0.2, zc), Vec3::Y, 0.7, 0.012, 2, (x * 13.0) as u32, false, [0.15, 0.13, 0.11]);
        }
        x += 2.2;
    }
    for (k, &(x, z, h)) in TREES.iter().enumerate() {
        tree(b, v3(x, GROUND_Y, z), h, false, k as u32 * 11 + 3, [0.17, 0.15, 0.13]);
    }

    // ---- стоянка через проезд: машины под снегом, фонарь ----
    for &(cx, cz, yaw, col) in &CARS {
        car(b, cx, cz, GROUND_Y, yaw, col);
    }
    let (lx, lz) = LAMP_POST;
    // бетонная опора, консоль к проезду, светильник ЖКУ
    b.cyl(Mat::Concrete, v3(lx, GROUND_Y, lz), 8.2, 0.12, 12, [0.6; 3]);
    b.tube(Mat::Metal, v3(lx, 8.0, lz), v3(lx, 8.25, lz + 1.4), 0.035, 1.0, 10, [0.35; 3]);
    b.ellipsoid(Mat::Metal, v3(lx, 8.25, lz + 1.5), v3(0.2, 0.14, 0.38), true, [0.3, 0.31, 0.32]);
    b.ellipsoid(Mat::Snow, v3(lx, 8.31, lz + 1.5), v3(0.15, 0.1, 0.3), true, [1.0; 3]);
    b.lamps.push(Lamp { pos: v3(lx, 8.2, lz + 1.5), candela: 4000.0, kelvin: 2000.0, kind: LampKind::Steady, bulb_r: 0.06, cone: Some((1.22, 0.85)), clear: false, range: 0.0 });

    // ---- кирпичная ограда с белёными арками, за ней гаражи ----
    // ИЗМЕНЕНО (текстуры двора): кирпич — своей текстурой (tex::brick)
    let brick = [1.0; 3];
    let (fx0, fx1) = (HOUSE_X0 - 6.0, 17.0);
    let bays = ((fx1 - fx0) / 2.9).floor() as i32;
    for i in 0..=bays {
        let x = fx0 + i as f32 * 2.9;
        b.cube(Mat::Brick, v3(x - 0.25, GROUND_Y, FENCE_Z - 0.25), v3(x + 0.25, GROUND_Y + 2.3, FENCE_Z + 0.25), 1.04, brick);
        b.cube(Mat::Metal, v3(x - 0.3, GROUND_Y + 2.3, FENCE_Z - 0.3), v3(x + 0.3, GROUND_Y + 2.36, FENCE_Z + 0.3), 0.5, [0.3; 3]);
        b.mound(Mat::Snow, v3(x, GROUND_Y + 2.36, FENCE_Z), v3(0.3, 0.1, 0.3), [1.0; 3]);
        if i == bays {
            break;
        }
        // пролёт: кирпичный низ, белёная вставка, арка из кирпича
        let (xa, xb) = (x + 0.25, x + 2.65);
        b.cube(Mat::Brick, v3(xa, GROUND_Y, FENCE_Z - 0.12), v3(xb, GROUND_Y + 0.5, FENCE_Z + 0.12), 1.04, brick);
        b.mound(Mat::Snow, v3((xa + xb) * 0.5, GROUND_Y + 0.5, FENCE_Z), v3(1.2, 0.08, 0.14), [1.0; 3]);
        // белёная вставка с аркой поверху и кирпич над аркой — вертикальными
        // полосами: у каждой своя высота арки (полуэллипс)
        let (cx, r) = ((xa + xb) * 0.5, (xb - xa) * 0.5);
        let arch = |x: f32| GROUND_Y + 1.75 + r * 0.45 * (1.0 - ((x - cx) / r).powi(2)).max(0.0).sqrt();
        let strips = 14;
        for k in 0..strips {
            let (x0, x1) = (xa + (xb - xa) * k as f32 / strips as f32, xa + (xb - xa) * (k + 1) as f32 / strips as f32);
            let (y0, y1) = (arch(x0), arch(x1));
            b.quad_w(Mat::White, [v3(x1, GROUND_Y + 0.5, FENCE_Z + 0.05), v3(x0, GROUND_Y + 0.5, FENCE_Z + 0.05), v3(x0, y0, FENCE_Z + 0.05), v3(x1, y1, FENCE_Z + 0.05)], Vec3::Z, 2.0, [0.85, 0.83, 0.78]);
            b.cube(Mat::Brick, v3(x0, y0.min(y1), FENCE_Z - 0.12), v3(x1, GROUND_Y + 2.15, FENCE_Z + 0.04), 1.04, brick);
        }
        // обрамление арки — кирпич чуть выступает
        let segs = 7;
        for sgm in 0..segs {
            let a0 = std::f32::consts::PI * sgm as f32 / segs as f32;
            let a1 = std::f32::consts::PI * (sgm + 1) as f32 / segs as f32;
            let p0 = v3(cx + r * a0.cos(), GROUND_Y + 1.75 + r * 0.45 * a0.sin(), FENCE_Z + 0.07);
            let p1 = v3(cx + r * a1.cos(), GROUND_Y + 1.75 + r * 0.45 * a1.sin(), FENCE_Z + 0.07);
            b.beam(Mat::Brick, p0, p1, Vec3::Z, 0.08, 0.1, brick);
        }
    }
    // гаражи за оградой: ряд железных боксов, у каждого свой цвет, снег на крыше
    let mut gx = fx0 + 1.0;
    let mut gi = 0;
    while gx < 40.0 {
        let col = [[0.28, 0.33, 0.25], [0.35, 0.22, 0.14], [0.3, 0.3, 0.32], [0.2, 0.25, 0.33]][gi % 4];
        let (ga, gb) = (gx, gx + 3.2);
        b.cube(Mat::Steel, v3(ga, GROUND_Y, -20.0), v3(gb - 0.05, GROUND_Y + 2.4, -14.0), 1.0, col);
        b.cube(Mat::Snow, v3(ga, GROUND_Y + 2.4, -20.0), v3(gb - 0.05, GROUND_Y + 2.55, -13.9), 4.0, [1.0; 3]);
        b.cube(Mat::Black, v3(ga + 0.3, GROUND_Y, -14.02), v3(gb - 0.35, GROUND_Y + 2.1, -14.0), 0.1, [0.03; 3]);
        b.cube(Mat::Steel, v3(ga + 0.32, GROUND_Y + 0.02, -14.06), v3(gb - 0.37, GROUND_Y + 2.08, -14.02), 1.0, [col[0] * 0.9, col[1] * 0.9, col[2] * 0.9]);
        gx += 3.2;
        gi += 1;
    }

    // ---- контейнерная площадка: плита, стенки из профлиста, три бака ----
    let [tx0, tz0, tx1, tz1] = TRASH;
    b.cube(Mat::Concrete, v3(tx0, GROUND_Y, tz0), v3(tx1, GROUND_Y + 0.08, tz1), 1.0, [0.5; 3]);
    let sheet = [0.32, 0.4, 0.34];
    b.cube(Mat::Sheet, v3(tx0, GROUND_Y, tz0), v3(tx1, GROUND_Y + 1.6, tz0 + 0.04), 0.46, sheet);
    b.cube(Mat::Sheet, v3(tx0, GROUND_Y, tz0), v3(tx0 + 0.04, GROUND_Y + 1.6, tz1), 0.46, sheet);
    b.cube(Mat::Sheet, v3(tx1 - 0.04, GROUND_Y, tz0), v3(tx1, GROUND_Y + 1.6, tz1), 0.46, sheet);
    for k in 0..3 {
        let cx = tx0 + 0.9 + k as f32 * 1.6;
        let col = if k == 1 { [0.12, 0.2, 0.42] } else { [0.12, 0.3, 0.15] };
        b.cube(Mat::Steel, v3(cx - 0.65, GROUND_Y + 0.08, tz0 + 0.15), v3(cx + 0.65, GROUND_Y + 1.15, tz0 + 1.05), 1.0, col);
        // крышка приоткрыта у среднего — торчит пакет
        if k == 1 {
            b.beam(Mat::Metal, v3(cx - 0.65, GROUND_Y + 1.18, tz0 + 0.2), v3(cx + 0.65, GROUND_Y + 1.18, tz0 + 0.2), Vec3::Y, 0.4, 0.03, col);
            b.ellipsoid(Mat::Black, v3(cx + 0.2, GROUND_Y + 1.18, tz0 + 0.7), v3(0.22, 0.2, 0.2), false, [0.02; 3]);
        } else {
            b.cube(Mat::Metal, v3(cx - 0.67, GROUND_Y + 1.15, tz0 + 0.13), v3(cx + 0.67, GROUND_Y + 1.19, tz0 + 1.07), 0.5, col);
            b.cube(Mat::Snow, v3(cx - 0.63, GROUND_Y + 1.19, tz0 + 0.16), v3(cx + 0.63, GROUND_Y + 1.25, tz0 + 1.04), 4.0, [1.0; 3]);
        }
    }
    // пакеты у баков
    for (k, (dx, dz)) in [(0.4f32, 1.3f32), (2.7, 1.45), (4.3, 1.25)].into_iter().enumerate() {
        b.ellipsoid(Mat::Black, v3(tx0 + dx, GROUND_Y + 0.08, tz0 + dz), v3(0.25, 0.22 + 0.05 * k as f32, 0.22), true, [0.02; 3]);
    }

    // ---- следы жизни: объявления у подъездов и на столбе, надписи краской ----
    // листок A4 на вертикальной плоскости с нормалью n (вдоль — `along`)
    let note = |b: &mut Building, o: Vec3, n: Vec3, along: Vec3, tilt: f32, age: f32| {
        let (hw, hh) = (0.105, 0.1485);
        let (c, sn) = (tilt.cos(), tilt.sin());
        let u = along * c + Vec3::Y * sn;
        let v = -along * sn + Vec3::Y * c;
        let col = [1.0 - age * 0.15, 0.98 - age * 0.22, 0.92 - age * 0.4];
        b.quad4(Mat::Paper, [o - u * hw - v * hh, o + u * hw - v * hh, o + u * hw + v * hh, o - u * hw + v * hh], [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]], n, col);
    };
    for (k, &c) in ENTRANCES.iter().enumerate() {
        let cnt = 2 + (hash01(k as i32, 501) * 3.0) as i32;
        for j in 0..cnt {
            let h = |m: i32| hash01(k as i32 * 17 + j, 503 + m);
            // по обе стороны двери, на уровне глаз; кто-то клеит прямо на домофон
            let side = if j % 2 == 0 { -1.0 } else { 1.0 };
            let x = c + side * (0.72 + 0.35 * h(1));
            let y = 1.25 + 0.5 * h(2);
            note(b, v3(x, y, zf - 0.004 - 0.001 * j as f32), -Vec3::Z, -Vec3::X, (h(3) - 0.5) * 0.25, h(4));
        }
    }
    for j in 0..3 {
        let h = |m: i32| hash01(j, 521 + m);
        let a = std::f32::consts::PI * (0.6 + 0.5 * j as f32);
        let n = v3(a.sin(), 0.0, a.cos());
        let o = v3(LAMP_POST.0, GROUND_Y + 1.35 + 0.3 * j as f32, LAMP_POST.1) + n * 0.125;
        note(b, o, n, n.cross(Vec3::Y), (h(1) - 0.5) * 0.3, 0.3 + 0.6 * h(2));
    }
    // надписи краской: ворота гаражей и белёные вставки ограды
    let inks = [[0.02, 0.02, 0.02], [0.05, 0.08, 0.3], [0.3, 0.03, 0.03]];
    let tag = |b: &mut Building, x0: f32, y0: f32, z: f32, w: f32, h: f32, var: u32, ink: [f32; 3]| {
        let u0 = var as f32 * 0.5;
        b.quad4(Mat::Scribble, [v3(x0, y0, z), v3(x0 + w, y0, z), v3(x0 + w, y0 + h, z), v3(x0, y0 + h, z)], [[u0 + 0.5, 1.0], [u0, 1.0], [u0, 0.0], [u0 + 0.5, 0.0]], Vec3::Z, ink);
    };
    let mut gx = HOUSE_X0 - 6.0 + 1.0;
    let mut gi = 0;
    while gx < 40.0 {
        if hash01(gi, 531) < 0.45 {
            tag(b, gx + 0.5 + 0.8 * hash01(gi, 533), GROUND_Y + 0.6 + 0.5 * hash01(gi, 535), -14.07, 1.4, 0.7, (gi % 2) as u32, inks[(hash01(gi, 537) * 3.0) as usize % 3]);
        }
        gx += 3.2;
        gi += 1;
    }
    let bays = ((17.0 - (HOUSE_X0 - 6.0)) / 2.9).floor() as i32;
    for i in 0..bays {
        if hash01(i, 541) < 0.35 {
            let x = HOUSE_X0 - 6.0 + i as f32 * 2.9 + 0.5;
            tag(b, x, GROUND_Y + 0.7, FENCE_Z + 0.055, 1.6, 0.8, (i % 2) as u32, inks[(hash01(i, 543) * 3.0) as usize % 3]);
        }
    }

    // ---- декали на фасаде (движковые, без геометрии): потёки, ржавчина,
    // копоть над лампами, надписи на цоколе, трещины штукатурки ----
    let spot = |b: &mut Building, cell: usize, sub: [f32; 4], center: Vec3, normal: Vec3, up: Vec3, w: f32, h: f32, color: [f32; 4]| {
        b.decals.push(DecalSpot { cell, sub, center, normal, up, w, h, color, roughness: -1.0 });
    };
    let full = [0.0, 0.0, 1.0, 1.0];
    let mut window_x = Vec::new();
    for (si, &c) in ENTRANCES.iter().enumerate() {
        for (wi, dx) in [-5.0f32, -2.6, 2.6, 5.0].into_iter().enumerate() {
            window_x.push(c + dx);
            for f in 0..FLOORS {
                let (i, j) = ((si * 4 + wi) as i32, f as i32);
                if hash01(i + 601, j + 7) < 0.55 {
                    let wy0 = f as f32 * H + 0.85;
                    let h = 0.8 + 0.6 * hash01(i + 603, j);
                    let k = 0.55 + 0.4 * hash01(i + 605, j);
                    spot(b, 2, full, v3(c + dx, wy0 - 0.05 - h * 0.5, zf), -Vec3::Z, Vec3::Y, 1.5, h, [1.0, 1.0, 1.0, k]);
                }
            }
        }
        // копоть на козырьке над лампой
        spot(b, 1, full, v3(c, 2.55, zf - 0.3), -Vec3::Y, -Vec3::Z, 0.8, 0.8, [1.0, 1.0, 1.0, 0.9]);
        // надписи на цоколе по сторонам от крыльца
        for (t, side) in [-1.0f32, 1.0].into_iter().enumerate() {
            if hash01(si as i32, 611 + t as i32) < 0.6 {
                let half = (hash01(si as i32, 613 + t as i32) * 2.0) as usize % 2;
                let sub = [half as f32 * 0.5, 0.0, half as f32 * 0.5 + 0.5, 1.0];
                let ink = [[0.15, 0.15, 0.17], [0.15, 0.25, 0.75], [0.7, 0.1, 0.08]][(hash01(si as i32, 615 + t as i32) * 3.0) as usize % 3];
                spot(b, 0, sub, v3(c + side * (1.9 + 0.8 * hash01(si as i32, 617 + t as i32)), 0.05, zf - 0.04), -Vec3::Z, Vec3::Y, 1.3, 0.62, [ink[0], ink[1], ink[2], 0.9]);
            }
        }
    }
    // ржавчина под кронштейнами газовой трубы (кроме тех, что над окнами)
    let mut bx = x0h + 1.0;
    let mut k = 0;
    while bx < x1h - 0.5 {
        let over_window = window_x.iter().any(|&wx| (bx - wx).abs() < 0.85);
        if !over_window && hash01(k, 621) < 0.55 {
            spot(b, 3, full, v3(bx, py - 0.07 - 0.4, zf), -Vec3::Z, Vec3::Y, 0.3, 0.8, [1.0, 1.0, 1.0, 0.85]);
        }
        bx += 2.0;
        k += 1;
    }
    // трещины штукатурки у земли
    for k in 0..14 {
        let x = x0h + 1.0 + (x1h - x0h - 2.0) * hash01(k, 631);
        if ENTRANCES.iter().any(|&c| (x - c).abs() < 1.6) {
            continue;
        }
        spot(b, 4, full, v3(x, 0.9 + 0.6 * hash01(k, 633), zf), -Vec3::Z, Vec3::Y, 1.2, 1.2, [0.55, 0.5, 0.45, 0.8]);
    }

    // ---- дома вдали за гаражами: фасады с окнами (силуэт и свет окон) ----
    let zd = -36.0;
    let yt = GROUND_Y + 0.6 + 5.0 * 2.8;
    b.quad_ws(Mat::Facade, [v3(-45.0, GROUND_Y, zd), v3(60.0, GROUND_Y, zd), v3(60.0, yt, zd), v3(-45.0, yt, zd)], Vec3::Z, [3.2, 2.8], [0.0, 0.0], [0.8, 0.78, 0.74]);
    for j in 0..5 {
        for i in 0..32 {
            let x0 = -44.0 + i as f32 * 3.2;
            let y0 = GROUND_Y + 0.6 + j as f32 * 2.8;
            let (wx0, wx1, wy0, wy1) = (x0 + 0.85, x0 + 2.35, y0 + 0.8, y0 + 2.2);
            let z = zd + 0.02;
            let q = [v3(wx0, wy0, z), v3(wx1, wy0, z), v3(wx1, wy1, z), v3(wx0, wy1, z)];
            if hash01(i + 300, j + 31) < 0.25 {
                let t = [[1.0, 0.3, 0.07], [1.0, 0.5, 0.12], [1.0, 0.62, 0.3], [0.9, 0.22, 0.1]][(hash01(i + 301, j) * 4.0) as usize % 4];
                let k = (0.1 + 0.25 * hash01(i + 302, j + 1)) * 4.5;
                let var = (hash01(i + 303, j + 2) * 4.0).floor() / 4.0;
                b.quad4(Mat::WinLit, q, [[var, 1.0], [var + 0.25, 1.0], [var + 0.25, 0.0], [var, 0.0]], Vec3::Z, [t[0] * k, t[1] * k, t[2] * k]);
            } else {
                b.quad_w(Mat::WinDark, q, Vec3::Z, 1.0, [1.0; 3]);
            }
        }
    }
}

/// ДОБАВЛЕНО (износ стен клетки, forward-декали): затёртая полоса на высоте
/// плеча вдоль маршей (марш A — у правой стены, B — у левой), грязь у
/// плинтуса вдоль маршей и площадок, засаленная стена вокруг выключателей.
fn wear_decals(b: &mut Building) {
    let full = [0.0, 0.0, 1.0, 1.0];
    let run = MID_Z - LAND_Z;
    let len = (run * run + HALF * HALF).sqrt();
    for n in 0..FLOORS - 1 {
        // (стена x, нормаль, высота у начала марша, направление по z)
        for (x, nx, y0, dir) in [(W, -1.0f32, landing_y(n), 1.0f32), (0.0, 1.0, mid_y(n), -1.0)] {
            let normal = v3(nx, 0.0, 0.0);
            let along = v3(0.0, HALF, run * dir).normalize();
            let up = normal.cross(along);
            let up = if up.y < 0.0 { -up } else { up };
            let zc = (LAND_Z + MID_Z) * 0.5;
            let yc = y0 + HALF * 0.5;
            let k = 0.7 + 0.3 * hash01(n as i32, x as i32 + 701);
            // плечо — ~1.25 м над проступью, полоса шириной 0.5 м
            b.decals.push(DecalSpot { cell: 5, sub: full, center: v3(x, yc + 1.25, zc), normal, up, w: len, h: 0.5, color: [1.0, 1.0, 1.0, k], roughness: -1.0 });
            // у плинтуса над ступенями (над носками ~0.1 м)
            b.decals.push(DecalSpot { cell: 6, sub: [0.0, 0.0, 1.0, 1.0], center: v3(x, yc + 0.12, zc), normal, up, w: len, h: 0.28, color: [1.0, 1.0, 1.0, 0.8], roughness: -1.0 });
        }
    }
    // плинтус площадок: вдоль левой и правой стен
    for n in 0..FLOORS {
        let y = landing_y(n);
        for (x, nx) in [(0.0f32, 1.0f32), (W, -1.0)] {
            b.decals.push(DecalSpot { cell: 6, sub: [0.0, 0.0, 1.0, 1.0], center: v3(x, y + 0.13, LAND_Z * 0.5), normal: v3(nx, 0.0, 0.0), up: Vec3::Y, w: LAND_Z, h: 0.26, color: [1.0, 1.0, 1.0, 0.75], roughness: -1.0 });
        }
        // засаленное пятно вокруг выключателя (на передней стене)
        let sx = if n == 0 { 0.55 } else { 0.62 };
        b.decals.push(DecalSpot { cell: 1, sub: full, center: v3(sx, y + 1.44, 0.0), normal: Vec3::Z, up: Vec3::Y, w: 0.42, h: 0.5, color: [0.9, 0.85, 0.8, 0.55], roughness: 0.45 });
    }
}
