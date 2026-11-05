//! Деформируемое тело из узлов, балок и шарниров: для просмотра разрушений при
//! ударе. Без GPU и движка — только расчёт, поэтому проверяется тестами.
//!
//! Узел — точечная масса. Балка работает на растяжение и сжатие и ведёт себя как
//! тонкостенный металл: сначала упруго, затем течёт, и новая длина остаётся (вмятина
//! не выправляется). На сжатии, потеряв устойчивость, держит меньшую силу (складывается
//! гармошкой), в конце сминается в плотный пакет; на растяжении рвётся. Шарнир
//! держит изгиб между двумя балками и тоже течёт (пластический шарнир).
//!
//! Схема — явная (симплектический Эйлер) с мелким шагом. Упругая жёсткость балок
//! ограничена устойчивостью шага и бывает ниже настоящей. На результат это почти не
//! влияет: при ударе энергию забирает течение, а его определяют силы текучести, они
//! настоящие. Упругий ход при таком ограничении — миллиметры.
//!
//! Весь расчёт ведёт баланс энергии: кинетическая + упругая + пластическая + потери
//! должны оставаться равны начальной плюс работа тяжести. Это главная проверка.

use std::ops::{Add, AddAssign, Mul, Neg, Sub, SubAssign};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct V3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

pub const fn v3(x: f64, y: f64, z: f64) -> V3 {
    V3 { x, y, z }
}

impl Add for V3 {
    type Output = V3;
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}
impl Sub for V3 {
    type Output = V3;
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}
impl Neg for V3 {
    type Output = V3;
    fn neg(self) -> V3 {
        v3(-self.x, -self.y, -self.z)
    }
}
impl Mul<f64> for V3 {
    type Output = V3;
    fn mul(self, k: f64) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
}
impl AddAssign for V3 {
    fn add_assign(&mut self, o: V3) {
        *self = *self + o;
    }
}
impl SubAssign for V3 {
    fn sub_assign(&mut self, o: V3) {
        *self = *self - o;
    }
}
impl V3 {
    pub fn dot(self, o: V3) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }
    pub fn cross(self, o: V3) -> V3 {
        v3(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }
    pub fn len(self) -> f64 {
        self.dot(self).sqrt()
    }
}

/// Сталь: модуль упругости, Па.
pub const STEEL_E: f64 = 210e9;

/// Как балка держит нагрузку. Силы — в ньютонах.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Section {
    /// Жёсткость сечения E*A, Н (упругая жёсткость балки — E*A / длина).
    pub ea: f64,
    /// Сила текучести на растяжении.
    pub yield_tension: f64,
    /// Рост силы текучести на единицу пластической деформации (упрочнение).
    pub hardening: f64,
    /// Предел на сжатии до потери устойчивости.
    pub peak_compression: f64,
    /// Средняя сила при складывании после потери устойчивости.
    pub plateau_compression: f64,
    /// Деформация на растяжении, при которой балка рвётся.
    pub rupture_strain: f64,
    /// Доля длины, смятая до плотного пакета: дальше сила резко растёт.
    pub bottom_strain: f64,
    /// Хрупкая (стекло): рвётся и на сжатии, при той же доле длины.
    pub brittle: bool,
}

impl Section {
    /// Тонкостенная квадратная труба: сторона b, стенка t, предел текучести sy.
    /// Средняя сила смятия — Abramowicz, Jones (1984): 13.06 sy t^(5/3) b^(1/3).
    pub fn square_tube(b: f64, t: f64, sy: f64) -> Section {
        let area = 4.0 * b * t;
        let plateau = 13.06 * sy * t.powf(5.0 / 3.0) * b.powf(1.0 / 3.0);
        Section {
            ea: STEEL_E * area,
            yield_tension: sy * area,
            hardening: 0.5 * sy * area,
            // пик первой складки: у деталей с насечками-инициаторами чуть выше плато
            peak_compression: (1.3 * plateau).min(sy * area),
            plateau_compression: plateau.min(sy * area),
            rupture_strain: 0.25,
            bottom_strain: 0.72,
            brittle: false,
        }
    }

    /// Полоса листа шириной w, толщиной t: на растяжении держит как металл, на сжатии
    /// сразу теряет устойчивость (критическое напряжение пластины, k = 4, но не выше sy).
    pub fn sheet(w: f64, t: f64, sy: f64) -> Section {
        let area = w * t;
        let sigma_cr = 4.0 * std::f64::consts::PI.powi(2) * STEEL_E / (12.0 * (1.0 - 0.09)) * (t / w).powi(2);
        let peak = sigma_cr.min(sy) * area;
        Section {
            ea: STEEL_E * area,
            yield_tension: sy * area,
            hardening: 0.5 * sy * area,
            peak_compression: peak,
            plateau_compression: 0.6 * peak,
            rupture_strain: 0.2,
            bottom_strain: 0.72,
            brittle: false,
        }
    }

    /// Закалённое/ламинированное стекло: почти не течёт, рвётся при малой деформации.
    pub fn glass(w: f64, t: f64) -> Section {
        let area = w * t;
        // прочность закалённого стекла порядка 100 МПа, модуль 70 ГПа
        let f = 100e6 * area;
        Section {
            ea: 70e9 * area,
            yield_tension: f,
            hardening: 0.0,
            peak_compression: f,
            plateau_compression: f,
            rupture_strain: 0.004,
            bottom_strain: 0.72,
            brittle: true,
        }
    }
}

// Beam, Hinge, Section, EnergyLedger — как типы в alkash3d-inertial/src/kernels/softbody.f90.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Beam {
    pub a: u32,
    pub b: u32,
    pub rest0: f64,
    pub rest: f64,
    pub k: f64,
    pub c: f64,
    pub section: Section,
    /// Потеряла устойчивость на сжатии: дальше держит только плато.
    pub buckled: bool,
    pub broken: bool,
    /// Пластическая деформация на растяжении (для упрочнения и разрыва).
    pub plastic_strain: f64,
}

/// Изгиб в узле m между балками m-a и m-c. Держит момент, больше — течёт.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct Hinge {
    pub a: u32,
    pub m: u32,
    pub c: u32,
    pub rest_angle: f64,
    pub k: f64,
    pub moment: f64,
    /// Наименьшие плечи для расчёта силы: половина исходных. Плечо смятой балки
    /// стремится к нулю, а сила момент / плечо — к бесконечности, и шаг теряет устойчивость.
    pub min_lever: [f64; 2],
    /// Балки, на которых держится шарнир: порвалась одна — шарнира нет.
    pub beams: [u32; 2],
    pub broken: bool,
}

/// Неподвижное препятствие.
#[derive(Clone, Copy, Debug)]
pub enum Obstacle {
    /// Жёсткий параллелепипед.
    Block { min: V3, max: V3 },
    /// Вертикальный жёсткий столб.
    Pole { x: f64, z: f64, radius: f64, height: f64 },
}

/// Препятствие в виде для ядра на Fortran (sb_obstacle): 0 — параллелепипед, 1 — столб.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct ObstacleC {
    pub kind: i32,
    pub min: V3,
    pub max: V3,
    pub x: f64,
    pub z: f64,
    pub radius: f64,
    pub height: f64,
}

/// Шаг на Fortran: softbody_step в alkash3d-inertial/src/kernels/softbody.f90.
#[allow(clippy::type_complexity)]
pub type StepKernel = unsafe extern "C" fn(
    n_nodes: i32,
    pos: *mut V3,
    vel: *mut V3,
    force: *mut V3,
    mass: *const f64,
    friction: *const f64,
    radius: *const f64,
    hits: *const bool,
    n_beams: i32,
    beams: *mut Beam,
    n_hinges: i32,
    hinges: *mut Hinge,
    n_obstacles: i32,
    obstacles: *const ObstacleC,
    n_pairs: i32,
    pairs: *const [u32; 2],
    self_radius: *const f64,
    n_pole: i32,
    pole: *const [u32; 2],
    has_ground: i32,
    ground_y: f64,
    gravity: f64,
    dt: f64,
    contact_k: f64,
    contact_c: f64,
    ledger: *mut EnergyLedger,
);

/// Куда пришла энергия с начала расчёта, Дж.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct EnergyLedger {
    pub plastic: f64,
    pub damping: f64,
    pub friction: f64,
    /// Работа, потерянная при разрыве балки (упругая энергия в момент разрыва).
    pub rupture: f64,
    pub contact_damping: f64,
}

impl EnergyLedger {
    pub fn total(&self) -> f64 {
        self.plastic + self.damping + self.friction + self.rupture + self.contact_damping
    }
}

pub struct SoftBody {
    pub pos: Vec<V3>,
    pub vel: Vec<V3>,
    pub mass: Vec<f64>,
    /// Коэффициент трения узла о препятствия.
    pub friction: Vec<f64>,
    /// Радиус узла при контакте с землёй и препятствиями.
    pub radius: Vec<f64>,
    /// Радиус узла при контакте с другими узлами (0 — не участвует).
    pub self_radius: Vec<f64>,
    /// Узел сталкивается с препятствиями (колёса при ударе о стену — нет).
    pub hits_obstacles: Vec<bool>,
    pub beams: Vec<Beam>,
    pub hinges: Vec<Hinge>,
    pub obstacles: Vec<Obstacle>,
    pub ground_y: Option<f64>,
    pub gravity: f64,
    pub dt: f64,
    pub time: f64,
    pub ledger: EnergyLedger,
    /// Балки, прямо касающиеся столба (столб тоньше шага сетки и прошёл бы между узлами).
    pub surface_beams: Vec<u32>,
    force: Vec<V3>,
    /// Пары узлов, соединённые балкой: им контакт не нужен.
    linked: std::collections::HashSet<(u32, u32)>,
    contact_k: f64,
    contact_c: f64,
    pairs: Vec<[u32; 2]>,
    pole_candidates: Vec<[u32; 2]>,
    obstacles_c: Vec<ObstacleC>,
    step_index: u64,
    /// Ядро на Fortran; None — эталонная реализация на Rust ниже.
    pub kernel: Option<StepKernel>,
}

const REBUILD_EVERY: u64 = 10;

impl SoftBody {
    pub fn new(dt: f64) -> Self {
        SoftBody {
            pos: Vec::new(),
            vel: Vec::new(),
            mass: Vec::new(),
            friction: Vec::new(),
            radius: Vec::new(),
            self_radius: Vec::new(),
            hits_obstacles: Vec::new(),
            beams: Vec::new(),
            hinges: Vec::new(),
            obstacles: Vec::new(),
            ground_y: None,
            gravity: 9.81,
            dt,
            time: 0.0,
            ledger: EnergyLedger::default(),
            surface_beams: Vec::new(),
            force: Vec::new(),
            linked: std::collections::HashSet::new(),
            contact_k: 0.0,
            contact_c: 0.0,
            pairs: Vec::new(),
            pole_candidates: Vec::new(),
            obstacles_c: Vec::new(),
            step_index: 0,
            kernel: None,
        }
    }

    pub fn add_node(&mut self, pos: V3, mass: f64) -> u32 {
        self.pos.push(pos);
        self.vel.push(V3::default());
        self.mass.push(mass);
        self.friction.push(0.5);
        self.radius.push(0.0);
        self.self_radius.push(0.0);
        self.hits_obstacles.push(true);
        (self.pos.len() - 1) as u32
    }

    pub fn add_beam(&mut self, a: u32, b: u32, section: Section) -> u32 {
        let l = (self.pos[b as usize] - self.pos[a as usize]).len().max(1e-4);
        self.beams.push(Beam { a, b, rest0: l, rest: l, k: section.ea / l, c: 0.0, section, buckled: false, broken: false, plastic_strain: 0.0 });
        (self.beams.len() - 1) as u32
    }

    /// Жёсткость `k` (Н*м/рад) и предельный момент (Н*м), балки — те, что сходятся в m.
    pub fn add_hinge(&mut self, a: u32, m: u32, c: u32, k: f64, moment: f64, beams: [u32; 2]) {
        let (u, v) = (self.pos[a as usize] - self.pos[m as usize], self.pos[c as usize] - self.pos[m as usize]);
        let angle = angle_between(u, v);
        self.hinges.push(Hinge { a, m, c, rest_angle: angle, k, moment, min_lever: [0.5 * u.len(), 0.5 * v.len()], beams, broken: false });
    }

    /// Ограничить жёсткости устойчивостью шага и задать затухание. Вызвать один раз
    /// после сборки. `damping` — доля критического затухания балки.
    pub fn finalize(&mut self, damping: f64) {
        let n = self.pos.len();
        let mut degree = vec![0u32; n];
        for b in &self.beams {
            degree[b.a as usize] += 1;
            degree[b.b as usize] += 1;
        }
        // сумма жёсткостей в узле / масса узла должна быть меньше (2/dt)^2 с запасом
        let limit = 0.25 * (2.0 / self.dt).powi(2);
        for b in &mut self.beams {
            let (ia, ib) = (b.a as usize, b.b as usize);
            let m_a = self.mass[ia] / degree[ia].max(1) as f64;
            let m_b = self.mass[ib] / degree[ib].max(1) as f64;
            b.k = b.k.min(limit * m_a.min(m_b));
            let reduced = self.mass[ia] * self.mass[ib] / (self.mass[ia] + self.mass[ib]);
            b.c = 2.0 * damping * (b.k * reduced).sqrt();
        }
        // шарнир действует на узел как жёсткость k / L^2
        for h in &mut self.hinges {
            let l1 = (self.pos[h.a as usize] - self.pos[h.m as usize]).len();
            let l2 = (self.pos[h.c as usize] - self.pos[h.m as usize]).len();
            let lever = l1.min(l2).powi(2);
            let m = self.mass[h.a as usize].min(self.mass[h.m as usize]).min(self.mass[h.c as usize]);
            h.k = h.k.min(0.05 * limit * m * lever);
        }
        let min_mass = self.mass.iter().cloned().fold(f64::MAX, f64::min);
        // контакт мягче балок: при ударе узел успевает затормозить за много шагов,
        // и работа сил считается точно
        self.contact_k = 0.02 * limit * min_mass;
        self.contact_c = 2.0 * 0.3 * (self.contact_k * min_mass).sqrt();
        self.force = vec![V3::default(); n];
        self.linked = self.beams.iter().map(|b| (b.a.min(b.b), b.a.max(b.b))).collect();
    }

    pub fn kinetic_energy(&self) -> f64 {
        self.vel.iter().zip(&self.mass).map(|(v, m)| 0.5 * m * v.dot(*v)).sum()
    }

    pub fn potential_energy(&self) -> f64 {
        self.pos.iter().zip(&self.mass).map(|(p, m)| m * self.gravity * p.y).sum()
    }

    pub fn elastic_energy(&self) -> f64 {
        let mut e = 0.0;
        for b in self.beams.iter().filter(|b| !b.broken) {
            let d = (self.pos[b.b as usize] - self.pos[b.a as usize]).len();
            let x = d - b.rest;
            e += 0.5 * b.k * x * x;
            let floor = b.rest0 * (1.0 - b.section.bottom_strain);
            if d < floor {
                e += 0.5 * dense_stiffness(b) * (floor - d).powi(2);
            }
        }
        for h in self.hinges.iter().filter(|h| !h.broken) {
            let a = angle_between(self.pos[h.a as usize] - self.pos[h.m as usize], self.pos[h.c as usize] - self.pos[h.m as usize]);
            e += 0.5 * h.k * (a - h.rest_angle).powi(2);
        }
        e
    }

    /// Вся механическая энергия плюс всё, что уже ушло в тепло и деформацию.
    pub fn energy_total(&self) -> f64 {
        self.kinetic_energy() + self.potential_energy() + self.elastic_energy() + self.ledger.total()
    }

    pub fn step(&mut self) {
        if self.step_index % REBUILD_EVERY == 0 {
            self.rebuild_candidates();
        }
        self.step_index += 1;
        if let Some(kernel) = self.kernel {
            // SAFETY: длины и указатели — из векторов этого тела; типы совпадают с softbody.f90
            unsafe {
                kernel(
                    self.pos.len() as i32,
                    self.pos.as_mut_ptr(),
                    self.vel.as_mut_ptr(),
                    self.force.as_mut_ptr(),
                    self.mass.as_ptr(),
                    self.friction.as_ptr(),
                    self.radius.as_ptr(),
                    self.hits_obstacles.as_ptr(),
                    self.beams.len() as i32,
                    self.beams.as_mut_ptr(),
                    self.hinges.len() as i32,
                    self.hinges.as_mut_ptr(),
                    self.obstacles_c.len() as i32,
                    self.obstacles_c.as_ptr(),
                    self.pairs.len() as i32,
                    self.pairs.as_ptr(),
                    self.self_radius.as_ptr(),
                    self.pole_candidates.len() as i32,
                    self.pole_candidates.as_ptr(),
                    self.ground_y.is_some() as i32,
                    self.ground_y.unwrap_or(0.0),
                    self.gravity,
                    self.dt,
                    self.contact_k,
                    self.contact_c,
                    &mut self.ledger,
                );
            }
            self.time += self.dt;
            return;
        }
        for f in &mut self.force {
            *f = V3::default();
        }
        self.apply_beams();
        self.apply_hinges();
        self.apply_contacts();
        // шаг: сначала скорость, потом положение (симплектический Эйлер)
        let dt = self.dt;
        for i in 0..self.pos.len() {
            let a = self.force[i] * (1.0 / self.mass[i]);
            self.vel[i] = self.vel[i] + (a - v3(0.0, self.gravity, 0.0)) * dt;
            self.pos[i] = self.pos[i] + self.vel[i] * dt;
        }
        self.time += dt;
    }

    fn apply_beams(&mut self) {
        let dt = self.dt;
        for i in 0..self.beams.len() {
            let mut b = self.beams[i];
            if b.broken {
                continue;
            }
            let (ia, ib) = (b.a as usize, b.b as usize);
            let d = self.pos[ib] - self.pos[ia];
            let len = d.len().max(1e-6);
            let n = d * (1.0 / len);
            let ext = len - b.rest;
            let mut f = b.k * ext;
            // скорость удлинения считаем заранее: работа течения — за фактическое удлинение
            let rate = (self.vel[ib] - self.vel[ia]).dot(n);
            if f > 0.0 {
                let limit = b.section.yield_tension + b.section.hardening * b.plastic_strain;
                if f > limit {
                    let flow = (f - limit) / b.k;
                    // работа течения — при силе предела: за один шаг перелёт упругого удлинения
                    // сверх предела — след дискретизации, кинетическая энергия его не оплачивала
                    self.ledger.plastic += limit * flow;
                    b.rest += flow;
                    b.plastic_strain += flow / b.rest0;
                    f = limit;
                }
            } else {
                let limit = if b.buckled { b.section.plateau_compression } else { b.section.peak_compression };
                if -f > limit {
                    let flow = (-f - limit) / b.k;
                    self.ledger.plastic += limit * flow;
                    b.rest -= flow;
                    if !b.buckled {
                        // сброс упругой энергии при переходе с пика на плато уходит в течение
                        let drop = (limit - b.section.plateau_compression).max(0.0) / b.k;
                        self.ledger.plastic += (limit + b.section.plateau_compression) * 0.5 * drop;
                        b.rest -= drop;
                        b.buckled = true;
                    }
                    f = -limit.min(b.section.plateau_compression);
                }
            }
            // смят в плотный пакет: материалу некуда деваться, сила резко растёт
            let floor = b.rest0 * (1.0 - b.section.bottom_strain);
            if len < floor {
                f -= dense_stiffness(&b) * (floor - len);
            }
            // разрыв: растянута больше предела, у стекла — и сжата
            let strain = (len - b.rest0) / b.rest0;
            if strain > b.section.rupture_strain || (b.section.brittle && strain < -b.section.rupture_strain) {
                let x = len - b.rest;
                self.ledger.rupture += 0.5 * b.k * x * x;
                if len < floor {
                    self.ledger.rupture += 0.5 * dense_stiffness(&b) * (floor - len).powi(2);
                }
                b.broken = true;
                self.beams[i] = b;
                continue;
            }
            let damp = b.c * rate;
            self.ledger.damping += damp * rate * dt;
            let total = n * (f + damp);
            self.force[ia] += total;
            self.force[ib] -= total;
            self.beams[i] = b;
        }
    }

    fn apply_hinges(&mut self) {
        for i in 0..self.hinges.len() {
            let mut h = self.hinges[i];
            if h.broken {
                continue;
            }
            if self.beams[h.beams[0] as usize].broken || self.beams[h.beams[1] as usize].broken {
                let a = angle_between(self.pos[h.a as usize] - self.pos[h.m as usize], self.pos[h.c as usize] - self.pos[h.m as usize]);
                self.ledger.rupture += 0.5 * h.k * (a - h.rest_angle).powi(2);
                self.hinges[i].broken = true;
                continue;
            }
            let (ia, im, ic) = (h.a as usize, h.m as usize, h.c as usize);
            let u = self.pos[ia] - self.pos[im];
            let v = self.pos[ic] - self.pos[im];
            let (lu, lv) = (u.len(), v.len());
            if lu < 1e-5 || lv < 1e-5 {
                continue;
            }
            let cross = u.cross(v);
            let cl = cross.len();
            // почти прямая или полностью сложенная линия: направление изгиба не определено
            if cl < 1e-9 * lu * lv {
                continue;
            }
            let angle = cl.atan2(u.dot(v));
            let mut torque = h.k * (angle - h.rest_angle);
            if torque.abs() > h.moment {
                let flow = (torque.abs() - h.moment) / h.k;
                self.ledger.plastic += h.moment * flow;
                h.rest_angle += flow * torque.signum();
                torque = h.moment * torque.signum();
                self.hinges[i] = h;
            }
            let n = cross * (1.0 / cl);
            // сила на концах плеч, перпендикулярно им, в сторону возврата угла к покою
            let fa = n.cross(u) * (torque / (lu * lu.max(h.min_lever[0])));
            let fc = v.cross(n) * (torque / (lv * lv.max(h.min_lever[1])));
            self.force[ia] += fa;
            self.force[ic] += fc;
            self.force[im] -= fa + fc;
        }
    }

    fn rebuild_candidates(&mut self) {
        self.obstacles_c.clear();
        for ob in &self.obstacles {
            self.obstacles_c.push(match *ob {
                Obstacle::Block { min, max } => ObstacleC { kind: 0, min, max, x: 0.0, z: 0.0, radius: 0.0, height: 0.0 },
                Obstacle::Pole { x, z, radius, height } => ObstacleC { kind: 1, min: V3::default(), max: V3::default(), x, z, radius, height },
            });
        }
        // пары узлов, которым грозит контакт между собой
        self.pairs.clear();
        let cell = 0.2;
        let mut grid: std::collections::HashMap<(i32, i32, i32), Vec<u32>> = std::collections::HashMap::new();
        let key = |p: V3| ((p.x / cell).floor() as i32, (p.y / cell).floor() as i32, (p.z / cell).floor() as i32);
        for i in 0..self.pos.len() {
            if self.self_radius[i] > 0.0 {
                grid.entry(key(self.pos[i])).or_default().push(i as u32);
            }
        }
        const MARGIN: f64 = 0.03;
        for i in 0..self.pos.len() {
            let ri = self.self_radius[i];
            if ri <= 0.0 {
                continue;
            }
            let reach = ((ri + 0.3 + MARGIN) / cell).ceil() as i32;
            let (cx, cy, cz) = key(self.pos[i]);
            for dx in -reach..=reach {
                for dy in -reach..=reach {
                    for dz in -reach..=reach {
                        let Some(list) = grid.get(&(cx + dx, cy + dy, cz + dz)) else { continue };
                        for &j in list {
                            if j as usize <= i || self.linked.contains(&(i as u32, j)) {
                                continue;
                            }
                            let dist = (self.pos[j as usize] - self.pos[i]).len();
                            if dist < ri + self.self_radius[j as usize] + MARGIN {
                                self.pairs.push([i as u32, j]);
                            }
                        }
                    }
                }
            }
        }
        // балки возле столбов
        self.pole_candidates.clear();
        for (o, ob) in self.obstacles.iter().enumerate() {
            if let Obstacle::Pole { x, z, radius, .. } = *ob {
                for &bi in &self.surface_beams {
                    let b = &self.beams[bi as usize];
                    if b.broken {
                        continue;
                    }
                    let (p, q) = (self.pos[b.a as usize], self.pos[b.b as usize]);
                    let d = segment_axis_distance(p, q, x, z).0;
                    if d < radius + 0.3 {
                        self.pole_candidates.push([bi, o as u32]);
                    }
                }
            }
        }
    }

    fn apply_contacts(&mut self) {
        let dt = self.dt;
        let (k, c) = (self.contact_k, self.contact_c);
        // земля и препятствия — по узлам
        for i in 0..self.pos.len() {
            let r = self.radius[i];
            let p = self.pos[i];
            if let Some(g) = self.ground_y {
                let depth = g + r - p.y;
                if depth > 0.0 {
                    self.contact_static(i, v3(0.0, 1.0, 0.0), depth, k, c, dt);
                }
            }
            if !self.hits_obstacles[i] {
                continue;
            }
            for o in 0..self.obstacles.len() {
                let contact = match self.obstacles[o] {
                    Obstacle::Block { min, max } => block_contact(p, r, min, max),
                    Obstacle::Pole { x, z, radius, height } => pole_contact(p, r, x, z, radius, height),
                };
                if let Some((normal, depth)) = contact {
                    self.contact_static(i, normal, depth, k, c, dt);
                }
            }
        }
        // столб тоньше шага сетки — ещё и по балкам обшивки
        for idx in 0..self.pole_candidates.len() {
            let [bi, o] = self.pole_candidates[idx];
            let o = o as usize;
            let b = self.beams[bi as usize];
            if b.broken {
                continue;
            }
            let Obstacle::Pole { x, z, radius, height } = self.obstacles[o] else { continue };
            let (ia, ib) = (b.a as usize, b.b as usize);
            let (d, t) = segment_axis_distance(self.pos[ia], self.pos[ib], x, z);
            if d >= radius || t <= 0.02 || t >= 0.98 {
                continue;
            }
            let point = self.pos[ia] * (1.0 - t) + self.pos[ib] * t;
            if point.y > height {
                continue;
            }
            let normal = {
                let n = v3(point.x - x, 0.0, point.z - z);
                n * (1.0 / n.len().max(1e-6))
            };
            let depth = radius - d;
            let vel = self.vel[ia] * (1.0 - t) + self.vel[ib] * t;
            let vn = vel.dot(normal);
            let fn_elastic = k * depth;
            let fn_damp = if vn < 0.0 { -c * vn } else { 0.0 };
            self.ledger.contact_damping += fn_damp * -vn.min(0.0) * dt;
            let f = normal * (fn_elastic + fn_damp);
            self.force[ia] += f * (1.0 - t);
            self.force[ib] += f * t;
            self.pole_elastic_correction(depth, k);
        }
        // узлы между собой
        for idx in 0..self.pairs.len() {
            let [a, b] = self.pairs[idx];
            let (ia, ib) = (a as usize, b as usize);
            let d = self.pos[ib] - self.pos[ia];
            let len = d.len().max(1e-6);
            let depth = self.self_radius[ia] + self.self_radius[ib] - len;
            if depth <= 0.0 {
                continue;
            }
            let n = d * (1.0 / len);
            let vn = (self.vel[ib] - self.vel[ia]).dot(n);
            let damp = if vn < 0.0 { -c * vn } else { 0.0 };
            self.ledger.contact_damping += damp * -vn.min(0.0) * dt;
            let f = n * (k * depth + damp);
            self.force[ia] -= f;
            self.force[ib] += f;
            self.ledger_elastic_pair(depth, k);
        }
    }

    // Упругая энергия контактов в балансе не хранится (мала и меняется быстро); она
    // возвращается в кинетическую, поэтому расхождение баланса на миг контакта — норма.
    fn pole_elastic_correction(&mut self, _depth: f64, _k: f64) {}
    fn ledger_elastic_pair(&mut self, _depth: f64, _k: f64) {}

    fn contact_static(&mut self, i: usize, normal: V3, depth: f64, k: f64, c: f64, dt: f64) {
        let v = self.vel[i];
        let vn = v.dot(normal);
        let fn_elastic = k * depth;
        let fn_damp = if vn < 0.0 { -c * vn } else { 0.0 };
        self.ledger.contact_damping += fn_damp * -vn.min(0.0) * dt;
        let fn_total = fn_elastic + fn_damp;
        let mut f = normal * fn_total;
        // трение Кулона со сглаживанием у нуля
        let vt = v - normal * vn;
        let speed = vt.len();
        if speed > 1e-6 {
            let mu = self.friction[i];
            let ft = (mu * fn_total).min(self.mass[i] * speed / dt * 0.5);
            f -= vt * (ft / speed);
            self.ledger.friction += ft * speed * dt;
        }
        self.force[i] += f;
    }
}

fn dense_stiffness(b: &Beam) -> f64 {
    // вдвое жёстче самой балки: больше не даёт устойчивость шага
    b.k * 2.0
}

fn angle_between(u: V3, v: V3) -> f64 {
    u.cross(v).len().atan2(u.dot(v))
}

fn block_contact(p: V3, r: f64, min: V3, max: V3) -> Option<(V3, f64)> {
    let inside = p.x > min.x - r && p.x < max.x + r && p.y > min.y - r && p.y < max.y + r && p.z > min.z - r && p.z < max.z + r;
    if !inside {
        return None;
    }
    let faces = [
        (p.x - (min.x - r), v3(-1.0, 0.0, 0.0)),
        ((max.x + r) - p.x, v3(1.0, 0.0, 0.0)),
        (p.y - (min.y - r), v3(0.0, -1.0, 0.0)),
        ((max.y + r) - p.y, v3(0.0, 1.0, 0.0)),
        (p.z - (min.z - r), v3(0.0, 0.0, -1.0)),
        ((max.z + r) - p.z, v3(0.0, 0.0, 1.0)),
    ];
    let mut best = faces[0];
    for f in &faces[1..] {
        if f.0 < best.0 {
            best = *f;
        }
    }
    Some((best.1, best.0))
}

fn pole_contact(p: V3, r: f64, x: f64, z: f64, radius: f64, height: f64) -> Option<(V3, f64)> {
    if p.y > height {
        return None;
    }
    let d = v3(p.x - x, 0.0, p.z - z);
    let len = d.len();
    let depth = radius + r - len;
    if depth <= 0.0 || len < 1e-6 {
        return None;
    }
    Some((d * (1.0 / len), depth))
}

/// Расстояние от отрезка до вертикальной оси (в плане) и параметр ближайшей точки.
fn segment_axis_distance(p: V3, q: V3, x: f64, z: f64) -> (f64, f64) {
    let (dx, dz) = (q.x - p.x, q.z - p.z);
    let l2 = dx * dx + dz * dz;
    let t = if l2 > 1e-12 { (((x - p.x) * dx + (z - p.z) * dz) / l2).clamp(0.0, 1.0) } else { 0.5 };
    let (cx, cz) = (p.x + dx * t, p.z + dz * t);
    (((cx - x).powi(2) + (cz - z).powi(2)).sqrt(), t)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn steel_bar() -> Section {
        Section { ea: 1e8, yield_tension: 1000.0, hardening: 0.0, peak_compression: 1000.0, plateau_compression: 500.0, rupture_strain: 10.0, bottom_strain: 0.9, brittle: false }
    }

    fn two_nodes(section: Section, dt: f64) -> SoftBody {
        let mut s = SoftBody::new(dt);
        s.gravity = 0.0;
        s.add_node(v3(0.0, 0.0, 0.0), 1.0);
        s.add_node(v3(1.0, 0.0, 0.0), 1.0);
        s.add_beam(0, 1, section);
        s.finalize(0.0);
        s
    }

    #[test]
    fn free_fall_matches_the_formula() {
        let mut s = SoftBody::new(1e-4);
        s.add_node(v3(0.0, 10.0, 0.0), 1.0);
        s.finalize(0.0);
        for _ in 0..10000 {
            s.step();
        }
        let expected = 10.0 - 0.5 * 9.81 * 1.0;
        assert!((s.pos[0].y - expected).abs() < 2e-3, "высота {}", s.pos[0].y);
    }

    #[test]
    fn elastic_bar_conserves_energy() {
        // шаг мелкий: у явной схемы измеренная энергия колеблется на ~ω*dt/2 вокруг точной
        let mut s = two_nodes(steel_bar(), 1e-6);
        s.beams[0].section.yield_tension = 1e12;
        s.beams[0].section.peak_compression = 1e12;
        s.beams[0].section.plateau_compression = 1e12;
        s.vel[1] = v3(0.5, 0.0, 0.0);
        s.vel[0] = v3(-0.5, 0.0, 0.0);
        let start = s.energy_total();
        let mut peak = 0.0f64;
        for _ in 0..200000 {
            s.step();
            peak = peak.max((s.energy_total() - start).abs());
        }
        assert!(peak < start * 0.01, "уход энергии {} из {}", peak, start);
    }

    #[test]
    fn tension_is_limited_by_yield_and_stays_stretched() {
        let mut s = two_nodes(steel_bar(), 1e-5);
        s.vel[1] = v3(5.0, 0.0, 0.0);
        s.vel[0] = v3(-5.0, 0.0, 0.0);
        let start = s.energy_total();
        for _ in 0..50000 {
            let before = s.vel[1].x;
            s.step();
            // настоящая приложенная сила — по изменению скорости (масса 1 кг)
            let force = (s.vel[1].x - before).abs() / s.dt;
            assert!(force <= 1000.0 + 1e-6, "сила {}", force);
        }
        // вся кинетическая: 2 * 0.5 * 1 * 25 = 25 Дж; течение при 1000 Н — 25 мм, не считая упругий отскок
        assert!(s.beams[0].rest > 1.0 + 0.015 && s.beams[0].rest < 1.03, "длина {}", s.beams[0].rest);
        assert!(s.ledger.plastic > 15.0 && s.ledger.plastic <= start * 1.01, "течение {}", s.ledger.plastic);
        assert!((s.energy_total() - start).abs() < start * 0.01, "баланс {} из {}", s.energy_total(), start);
    }

    #[test]
    fn compression_drops_to_the_plateau_after_buckling() {
        let mut s = two_nodes(steel_bar(), 1e-5);
        s.vel[1] = v3(-5.0, 0.0, 0.0);
        s.vel[0] = v3(5.0, 0.0, 0.0);
        let start = s.energy_total();
        let mut peak = 0.0f64;
        for _ in 0..50000 {
            let before = s.vel[1].x;
            s.step();
            peak = peak.max((s.vel[1].x - before).abs() / s.dt);
        }
        assert!(s.beams[0].buckled);
        assert!(peak <= 1000.0 + 1e-6, "пик {}", peak);
        // 25 Дж при 500 Н — около 50 мм смятия
        assert!(s.beams[0].rest < 1.0 - 0.03 && s.beams[0].rest > 1.0 - 0.06, "длина {}", s.beams[0].rest);
        assert!((s.energy_total() - start).abs() < start * 0.01, "баланс {} из {}", s.energy_total(), start);
    }

    #[test]
    fn beam_breaks_past_rupture_and_stops_pulling() {
        let mut section = steel_bar();
        section.rupture_strain = 0.01;
        let mut s = two_nodes(section, 1e-5);
        s.vel[1] = v3(5.0, 0.0, 0.0);
        s.vel[0] = v3(-5.0, 0.0, 0.0);
        for _ in 0..50000 {
            s.step();
        }
        assert!(s.beams[0].broken);
        assert!(s.vel[1].x > 0.0 && s.vel[0].x < 0.0, "после разрыва узлы разлетаются");
    }

    #[test]
    fn momentum_is_conserved_by_beams_and_hinges() {
        let mut s = SoftBody::new(1e-5);
        s.gravity = 0.0;
        let a = s.add_node(v3(-1.0, 0.1, 0.0), 1.0);
        let m = s.add_node(v3(0.0, 0.0, 0.0), 2.0);
        let c = s.add_node(v3(1.0, 0.2, 0.3), 3.0);
        let b0 = s.add_beam(a, m, steel_bar());
        let b1 = s.add_beam(m, c, steel_bar());
        s.add_hinge(a, m, c, 1000.0, 50.0, [b0, b1]);
        s.finalize(0.01);
        s.vel[0] = v3(1.0, 2.0, 0.0);
        s.vel[2] = v3(-1.0, 1.0, -3.0);
        let momentum = |s: &SoftBody| s.vel.iter().zip(&s.mass).fold(V3::default(), |acc, (v, m)| acc + *v * *m);
        let before = momentum(&s);
        for _ in 0..20000 {
            s.step();
        }
        assert!((momentum(&s) - before).len() < 1e-6, "импульс изменился");
    }

    #[test]
    fn hinge_straightens_elastically_and_keeps_a_plastic_bend() {
        // небольшой изгиб: упругий, энергия сохраняется
        let build = |dy: f64, moment: f64| {
            let mut s = SoftBody::new(1e-5);
            s.gravity = 0.0;
            let a = s.add_node(v3(-1.0, 0.0, 0.0), 1.0);
            let m = s.add_node(v3(0.0, 0.0, 0.0), 1.0);
            let c = s.add_node(v3(1.0, 0.0, 0.0), 1.0);
            let soft = Section { ea: 1e5, yield_tension: 1e12, peak_compression: 1e12, plateau_compression: 1e12, ..steel_bar() };
            let b0 = s.add_beam(a, m, soft);
            let b1 = s.add_beam(m, c, soft);
            s.add_hinge(a, m, c, 100.0, moment, [b0, b1]);
            s.finalize(0.0);
            s.pos[1].y = dy;
            s
        };
        let mut s = build(0.05, 1e6);
        let start = s.energy_total();
        assert!(s.elastic_energy() > 0.0);
        let mut drift = 0.0f64;
        for _ in 0..30000 {
            s.step();
            drift = drift.max((s.energy_total() - start).abs());
        }
        assert!(drift < start.max(1e-3) * 0.02, "уход {} из {}", drift, start);
        // сильный изгиб при малом моменте: угол покоя сместился, изгиб остался
        let mut s = build(0.3, 1.0);
        let before = s.hinges[0].rest_angle;
        for _ in 0..20000 {
            s.step();
        }
        assert!((s.hinges[0].rest_angle - before).abs() > 0.05, "пластического изгиба нет");
    }

    #[test]
    fn mass_stopped_by_a_crushing_tube_travels_the_energy_distance() {
        // масса 100 кг, 10 м/с (5 кДж), труба смятия с силой F: ход s = E / F
        let plateau = 50_000.0;
        let section = Section { ea: 2e8, yield_tension: 1e6, hardening: 0.0, peak_compression: plateau, plateau_compression: plateau, rupture_strain: 1.0, bottom_strain: 0.95, brittle: false };
        let mut s = SoftBody::new(2e-5);
        s.gravity = 0.0;
        let fixed = s.add_node(v3(0.0, 0.0, 0.0), 1e6);
        let moving = s.add_node(v3(1.0, 0.0, 0.0), 100.0);
        s.add_beam(moving, fixed, section);
        s.finalize(0.0);
        s.vel[1] = v3(-10.0, 0.0, 0.0);
        let mut least = 1.0f64;
        for _ in 0..100000 {
            s.step();
            least = least.min(s.pos[1].x);
        }
        let travel = 1.0 - least;
        assert!((travel - 0.1).abs() < 0.01, "ход {} м, по формуле 0.1 м", travel);
    }

    #[test]
    fn block_and_pole_stop_a_falling_node() {
        let mut s = SoftBody::new(1e-4);
        s.add_node(v3(0.0, 1.0, 0.0), 1.0);
        s.obstacles.push(Obstacle::Block { min: v3(-1.0, -1.0, -1.0), max: v3(1.0, 0.0, 1.0) });
        s.friction[0] = 0.0;
        s.finalize(0.0);
        for _ in 0..30000 {
            s.step();
        }
        assert!(s.pos[0].y > -0.02 && s.pos[0].y < 0.02, "высота {}", s.pos[0].y);
        let contact = pole_contact(v3(0.1, 0.5, 0.0), 0.0, 0.0, 0.0, 0.2, 2.0).unwrap();
        assert!((contact.1 - 0.1).abs() < 1e-9 && contact.0.x > 0.99);
        assert!(pole_contact(v3(0.1, 3.0, 0.0), 0.0, 0.0, 0.0, 0.2, 2.0).is_none());
    }

    #[test]
    fn sections_use_the_published_formula() {
        // Abramowicz-Jones, 90 мм, стенка 1.6 мм, 420 МПа: порядка 50 кН
        let s = Section::square_tube(0.09, 0.0016, 420e6);
        assert!(s.plateau_compression > 45e3 && s.plateau_compression < 60e3, "{}", s.plateau_compression);
        // лист теряет устойчивость намного раньше текучести
        let sheet = Section::sheet(0.2, 0.0008, 220e6);
        assert!(sheet.peak_compression < 0.1 * sheet.yield_tension);
    }
}
