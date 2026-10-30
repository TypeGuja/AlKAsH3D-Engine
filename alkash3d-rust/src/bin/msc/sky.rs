//! Солнце, небо и воздух — из одной физической модели атмосферы.
//!
//! Солнце стоит там, где оно реально стоит над Перяярви (≈61° с. ш.) в
//! начале июля: в полночь оно всего на ~6° под горизонтом, поэтому ночь —
//! белая, сумеречная. Атмосфера — сферический слой (Земля 6360 км, верх
//! 6420 км) с рэлеевским рассеянием (шкала высоты 8 км), аэрозолем Ми
//! (1.2 км) и поглощением озона (слой на 10–40 км, он даёт синеву сумерек).
//! Однократное рассеяние считается лучом по атмосфере (Nishita 1993) на
//! трёх длинах волн (680/550/440 нм). Многократное рассеяние — приближённо:
//! второй и следующие порядки как изотропный источник, освещённый средней
//! яркостью неба и земли с прошлого пересчёта (итерации сходятся за
//! несколько обновлений). Это приближение, а не точный перенос излучения.
//!
//! Из этой же модели берутся:
//! - цвет и сила солнца у земли (ослабление по пути луча, закат — красный);
//! - яркость неба по направлениям — купол из вершин вокруг камеры;
//! - рассеянный свет (ambient движка): небо сверху и свет, отражённый
//!   землёй снизу. Движок знает только равномерный ambient без зависимости
//!   от нормали, поэтому берётся смесь 65% неба + 35% земли — средняя
//!   освещённость для поверхностей разных наклонов (приближение);
//! - коэффициенты воздушной перспективы в шейдере (те же β, что у неба).
//!
//! Единицы: освещённость солнца за пределами атмосферы E0 = π, тогда
//! "интенсивность" солнца движка (I = E/π) вне атмосферы — 1, а все
//! яркости (небо, ambient) — в тех же единицах, что и освещённая поверхность.

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::Vec3;

const R_EARTH: f32 = 6_360_000.0;
const R_TOP: f32 = 6_420_000.0;
const H_R: f32 = 8_000.0;
const H_M: f32 = 1_200.0;
/// Рэлей у земли, 1/м (680, 550, 440 нм).
const BETA_R: [f32; 3] = [5.8e-6, 13.5e-6, 33.1e-6];
/// Аэрозоль: рассеяние у земли (летняя дымка над озёрами, видимость ~70 км).
const BETA_M: f32 = 3.0e-5;
const MIE_ABSORB: f32 = 1.1;
const MIE_G: f32 = 0.76;
/// Озон: поглощение в максимуме слоя, 1/м.
const BETA_O: [f32; 3] = [0.65e-6, 1.881e-6, 0.085e-6];
/// Угловой радиус Солнца.
const SUN_RADIUS: f32 = 0.004_65;
const LATITUDE: f32 = 61.0;
/// Склонение Солнца в начале июля, градусы.
const DECLINATION: f32 = 23.0;
/// Альбедо земли (лес, трава) — для лучей неба ниже горизонта.
const GROUND_ALBEDO: f32 = 0.15;

const AZ: usize = 48;
const EL: usize = 32;

type Rgb = [f32; 3];

fn mul(a: Rgb, k: f32) -> Rgb {
    [a[0] * k, a[1] * k, a[2] * k]
}

fn add(a: Rgb, b: Rgb) -> Rgb {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Плотности (Рэлей, Ми, озон) на высоте `h` над землёй.
fn density(h: f32) -> (f32, f32, f32) {
    let ozone = (1.0 - (h - 25_000.0).abs() / 15_000.0).max(0.0);
    ((-h / H_R).exp(), (-h / H_M).exp(), ozone)
}

/// Пересечение луча с концентрической сферой: дальний корень (или None).
fn sphere_far(o: Vec3, d: Vec3, r: f32) -> Option<f32> {
    let b = o.dot(d);
    let c = o.dot(o) - r * r;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    Some(-b + disc.sqrt())
}

/// Ближний положительный корень (луч упёрся в землю).
fn sphere_near(o: Vec3, d: Vec3, r: f32) -> Option<f32> {
    let b = o.dot(d);
    let c = o.dot(o) - r * r;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    (t > 0.0).then_some(t)
}

fn extinction(od: (f32, f32, f32)) -> Rgb {
    std::array::from_fn(|i| (-(BETA_R[i] * od.0 + BETA_M * MIE_ABSORB * od.1 + BETA_O[i] * od.2)).exp())
}

/// Оптическая толщина (в "метрах при плотности у земли") от точки `p` до верха атмосферы.
/// None — луч уходит в Землю (точка в её тени).
fn optical_depth_to_top(p: Vec3, d: Vec3, steps: usize) -> Option<(f32, f32, f32)> {
    if sphere_near(p, d, R_EARTH).is_some() {
        return None;
    }
    let len = sphere_far(p, d, R_TOP)?;
    let ds = len / steps as f32;
    let mut od = (0.0, 0.0, 0.0);
    for k in 0..steps {
        let q = p + d * ((k as f32 + 0.5) * ds);
        let (r, m, o) = density(q.length() - R_EARTH);
        od.0 += r * ds;
        od.1 += m * ds;
        od.2 += o * ds;
    }
    Some(od)
}

fn phase_r(c: f32) -> f32 {
    3.0 / (16.0 * std::f32::consts::PI) * (1.0 + c * c)
}

fn phase_m(c: f32) -> f32 {
    let g = MIE_G;
    (1.0 - g * g) / (4.0 * std::f32::consts::PI * (1.0 + g * g - 2.0 * g * c).max(1e-4).powf(1.5))
}

pub struct Sky {
    /// Направление НА солнце.
    pub sun_dir: Vec3,
    /// Интенсивность солнца у земли (движок: I = E/π), по каналам.
    pub sun: Rgb,
    /// Рассеянный свет движка (см. шапку): смесь неба и земли.
    pub ambient: Rgb,
    /// Освещённость от неба сверху / π (средняя яркость неба по cos).
    sky_avg: Rgb,
    /// Яркость земли (ламбертово альбедо под солнцем и небом).
    ground: Rgb,
    /// Экспозиция камеры (плавно адаптируется).
    pub exposure: f32,
    dome: [usize; 3],
    dome_slot: usize,
    sun_disk: usize,
    /// Высота глаза над уровнем моря модели (м).
    altitude: f32,
    last_hour: f32,
    radiance: Vec<Rgb>,
}

/// Направление на солнце (x — восток, y — вверх, -z — север: у мира Unity
/// север — +Z, после зеркалирования -Z) по местному солнечному времени.
pub fn sun_direction(hour: f32) -> Vec3 {
    let phi = LATITUDE.to_radians();
    let delta = DECLINATION.to_radians();
    let h = ((hour - 12.0) * 15.0).to_radians();
    let east = -delta.cos() * h.sin();
    let north = phi.cos() * delta.sin() - phi.sin() * delta.cos() * h.cos();
    let up = phi.sin() * delta.sin() + phi.cos() * delta.cos() * h.cos();
    Vec3::new(east, up, -north).normalize()
}

fn dome_dir(ia: usize, ie: usize) -> Vec3 {
    // Сгущение к горизонту: там цвет меняется быстрее всего.
    let t = ie as f32 / (EL - 1) as f32;
    let el = (t * 2.0 - 1.0).signum() * (t * 2.0 - 1.0).abs().powf(1.6) * std::f32::consts::FRAC_PI_2;
    let az = ia as f32 / AZ as f32 * std::f32::consts::TAU;
    Vec3::new(el.cos() * az.cos(), el.sin(), el.cos() * az.sin())
}

impl Sky {
    pub fn new(engine: &mut AlkashEngine) -> Sky {
        let mut indices = Vec::new();
        for ie in 0..EL - 1 {
            for ia in 0..AZ {
                let a = (ie * AZ + ia) as u32;
                let b = (ie * AZ + (ia + 1) % AZ) as u32;
                let c = a + AZ as u32;
                let d = b + AZ as u32;
                indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
        }
        let verts: Vec<Vertex> = (0..EL * AZ).map(|k| {
            let v = dome_dir(k % AZ, k / AZ);
            Vertex::with_normal(v.x, v.y, v.z, -v.x, -v.y, -v.z, 0.0, 0.0, 0.0, 1.0)
        }).collect();
        // Кольцо из трёх мешей: вершины одного меняются, пока GPU рисует другие.
        let dome = std::array::from_fn(|_| {
            let mut m = Mesh::from_vertices_and_indices(&verts, &indices).expect("купол неба");
            m.unlit = true;
            m.cpu_writable = true;
            engine.add_mesh(m)
        });
        // Диск солнца: круг в плоскости XY, лицом к +Z.
        let mut dv = vec![Vertex::with_normal(0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0)];
        let mut di = Vec::new();
        for k in 0..24 {
            let a = k as f32 / 24.0 * std::f32::consts::TAU;
            dv.push(Vertex::with_normal(a.cos(), a.sin(), 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0));
            di.extend_from_slice(&[0, 1 + k, 1 + (k + 1) % 24]);
        }
        let mut disk = Mesh::from_vertices_and_indices(&dv, &di).expect("диск солнца");
        disk.unlit = true;
        let sun_disk = engine.add_mesh(disk);
        Sky {
            sun_dir: Vec3::Y,
            sun: [0.0; 3],
            ambient: [0.0; 3],
            sky_avg: [0.0; 3],
            ground: [0.0; 3],
            exposure: 1.0,
            dome,
            dome_slot: 0,
            sun_disk,
            altitude: 120.0,
            last_hour: f32::NAN,
            radiance: vec![[0.0; 3]; EL * AZ],
        }
    }

    /// Яркость неба в направлении `d` (однократное рассеяние).
    fn sky_radiance(&self, d: Vec3) -> Rgb {
        let o = Vec3::new(0.0, R_EARTH + self.altitude, 0.0);
        let ground = sphere_near(o, d, R_EARTH);
        let Some(top) = sphere_far(o, d, R_TOP) else { return [0.0; 3] };
        let len = ground.unwrap_or(top);
        const STEPS: usize = 24;
        let ds = len / STEPS as f32;
        let cos = d.dot(self.sun_dir);
        let (pr, pm) = (phase_r(cos), phase_m(cos));
        let e0 = std::f32::consts::PI;
        let mut od = (0.0, 0.0, 0.0);
        let mut sum = [0.0f32; 3];
        let mut multi = [0.0f32; 3];
        // Изотропный источник многократного рассеяния: средняя яркость
        // окружения точки — полусфера неба сверху и земля снизу.
        let iso = mul(add(self.sky_avg, self.ground), 0.5);
        for k in 0..STEPS {
            let p = o + d * ((k as f32 + 0.5) * ds);
            let (r, m, oz) = density(p.length() - R_EARTH);
            od.0 += r * ds * 0.5;
            od.1 += m * ds * 0.5;
            od.2 += oz * ds * 0.5;
            let tv = extinction(od);
            for i in 0..3 {
                multi[i] += tv[i] * (BETA_R[i] * r + BETA_M * m) * iso[i] * ds;
            }
            if let Some(sod) = optical_depth_to_top(p, self.sun_dir, 8) {
                let t = extinction((od.0 + sod.0, od.1 + sod.1, od.2 + sod.2));
                for i in 0..3 {
                    sum[i] += t[i] * (BETA_R[i] * r * pr + BETA_M * m * pm) * ds;
                }
            }
            od.0 += r * ds * 0.5;
            od.1 += m * ds * 0.5;
            od.2 += oz * ds * 0.5;
        }
        let mut l = add(mul(sum, e0), multi);
        // Луч ниже горизонта упёрся в землю: её свет через воздух.
        if ground.is_some() {
            let t = extinction(od);
            l = add(l, [self.ground[0] * t[0], self.ground[1] * t[1], self.ground[2] * t[2]]);
        }
        l
    }

    /// Пересчёт солнца, неба и рассеянного света на час `hour`.
    pub fn update(&mut self, hour: f32) {
        if (hour - self.last_hour).abs() < 0.03 {
            return;
        }
        self.last_hour = hour;
        self.sun_dir = sun_direction(hour);
        let o = Vec3::new(0.0, R_EARTH + self.altitude, 0.0);
        self.sun = match optical_depth_to_top(o, self.sun_dir, 32) {
            Some(od) => extinction(od),
            // Солнце под горизонтом; верхний край диска ещё может светить —
            // упрощение: прямого света нет.
            None => [0.0; 3],
        };
        // Небо зависит от себя самого (земля, многократное рассеяние): при
        // первом расчёте — несколько итераций, дальше одна на обновление.
        let first = self.sky_avg == [0.0; 3];
        for pass in (if first { 0 } else { 3 })..4 {
            let mut e = [0.0f32; 3];
            let mut w_sum = 0.0;
            for ie in 0..EL {
                for ia in 0..AZ {
                    let d = dome_dir(ia, ie);
                    let l = if pass == 0 && d.y < 0.0 { [0.0; 3] } else { self.sky_radiance(d) };
                    self.radiance[ie * AZ + ia] = l;
                    if d.y > 0.0 {
                        // Телесный угол ячейки ∝ cos(el) * Δel; веса нормируем на ∫cos·dω = π.
                        let w = d.y * (1.0 - d.y * d.y).sqrt().max(0.05) * self.cell_band(ie);
                        e = add(e, mul(l, w));
                        w_sum += w;
                    }
                }
            }
            // ∫ L cosθ dω / π: средневзвешенная по cos яркость (ячейки покрывают полусферу).
            self.sky_avg = mul(e, 1.0 / w_sum.max(1e-6));
            self.ground = mul(add(mul(self.sun, self.sun_dir.y.max(0.0)), self.sky_avg), GROUND_ALBEDO);
            self.ambient = add(mul(self.sky_avg, 0.65), mul(self.ground, 0.35));
        }
    }

    fn cell_band(&self, ie: usize) -> f32 {
        let el = |k: f32| {
            let t = k / (EL - 1) as f32;
            (t * 2.0 - 1.0).signum() * (t * 2.0 - 1.0).abs().powf(1.6) * std::f32::consts::FRAC_PI_2
        };
        (el(ie as f32 + 0.5) - el(ie as f32 - 0.5)).abs()
    }

    /// Записать купол в следующий меш кольца и вернуть его инстанс + солнце.
    pub fn push(&mut self, engine: &mut AlkashEngine, out: &mut Vec<MeshInstance>) {
        let cam = engine.camera.position;
        let r = engine.camera.far * 0.9;
        self.dome_slot = (self.dome_slot + 1) % 3;
        let verts: Vec<Vertex> = (0..EL * AZ)
            .map(|k| {
                let v = dome_dir(k % AZ, k / AZ);
                let l = self.radiance[k];
                Vertex::with_normal(v.x, v.y, v.z, -v.x, -v.y, -v.z, l[0], l[1], l[2], 1.0)
            })
            .collect();
        let _ = engine.meshes[self.dome[self.dome_slot]].update_vertices(&verts);
        out.push(MeshInstance::new(self.dome[self.dome_slot]).at(cam.x, cam.y, cam.z).scaled(r, r, r));

        // Солнечный диск: яркость = освещённость / телесный угол диска.
        if self.sun_dir.y > -SUN_RADIUS * 2.0 && self.sun.iter().any(|&c| c > 0.0) {
            let omega = std::f32::consts::PI * SUN_RADIUS * SUN_RADIUS;
            let l = mul(self.sun, std::f32::consts::PI / omega);
            let dist = r * 0.98;
            let c = cam + self.sun_dir * dist;
            let size = dist * SUN_RADIUS;
            // Повернуть +Z диска к камере.
            let rot = alkash3d_rs::math::Quat::from_rotation_arc(Vec3::Z, -self.sun_dir);
            let (rz, ry, rx) = rot.to_euler(alkash3d_rs::math::EulerRot::ZYX);
            // Яркость через tint (меш белый); half-float таргет держит до 65504.
            let k = 60_000.0 / l.iter().cloned().fold(1.0, f32::max).max(60_000.0);
            out.push(MeshInstance::new(self.sun_disk).at(c.x, c.y, c.z).rotated(rx, ry, rz).scaled(size, size, size).tinted(l[0] * k, l[1] * k, l[2] * k, 1.0));
        }
    }

    /// Передать солнце, небо и воздух в движок (после `engine.update`, он
    /// пересчитывает своё простое солнце каждый кадр — перекрываем).
    pub fn apply(&mut self, engine: &mut AlkashEngine, dt: f32) {
        let tc = &mut engine.transform_constants;
        let d = -self.sun_dir;
        tc.light_dir = [d.x, d.y, d.z, 0.0];
        let peak = self.sun.iter().cloned().fold(0.0, f32::max);
        if peak > 1e-6 {
            tc.light_color = [self.sun[0] / peak, self.sun[1] / peak, self.sun[2] / peak, peak];
        } else {
            tc.light_color = [1.0, 1.0, 1.0, 0.0];
        }
        tc.ambient_color = [self.ambient[0], self.ambient[1], self.ambient[2], 1.0];
        tc.fog_scattering = [BETA_R[0], BETA_R[1], BETA_R[2], BETA_M];
        tc.fog_params = [H_M.max(2000.0), 0.0, MIE_G, 1.0];

        // Автоэкспозиция, как у глаза/камеры: средняя освещённость сцены
        // (солнце на 0.5 средней освещённой площади + небо), серая карта 18%.
        let lum = |c: Rgb| 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
        let scene = lum(self.sun) * self.sun_dir.y.max(0.0) * 0.5 + lum(self.ambient);
        // Средне-серая поверхность (18%) после экспозиции — около 0.18.
        // Ночью глаз не адаптируется бесконечно (сумеречное зрение): предел
        // ~120× от дневной, поэтому белая ночь остаётся сумеречной.
        let target = (1.0 / scene.max(1e-5)).min(300.0);
        let k = 1.0 - (-dt * 1.5).exp();
        self.exposure *= (target / self.exposure).powf(k);
        let settings = engine.light_global_settings.get_or_insert(alkash3d_rs::GlobalLightSettings {
            master_intensity: 1.0,
            shadow_quality: 2,
            shadow_distance: 200.0,
            cascade_count: 3,
            volumetric_enabled: 0,
            volumetric_quality: 0,
            bloom_intensity: 0.04,
            exposure: 1.0,
            gamma: 2.2,
        });
        settings.exposure = self.exposure;
        // Рассеяние в оптике глаза/объектива — малая доля яркого света.
        settings.bloom_intensity = 0.04;
    }
}
