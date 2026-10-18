//! Официальные паппеты DHX (пакет MLPO от `G:/mlp_2d/tools/export_official.py`).
//!
//! Персонаж — ракурсы из поворота на 360° в исходных .fla (ключевые кадры
//! метки "Rotation") плюс основная поза. Ракурс — части (голова, грива,
//! шея, торс, ноги, хвост) со своими текстурами; матрица из пакета уже
//! переводит пиксели части в метры персонажа, так что часть — квад в
//! плоскости фигуры. Порядок рисования — сдвиг по глубине на доли
//! миллиметра. Ракурс выбирается по углу между взглядом персонажа и
//! направлением на камеру (как спрайты в старых 3D-играх). У части фоновых
//! поворот нарисован только на пол-оборота (анфас — бок — спина, углы 0..π):
//! второй бок — те же ракурсы, отражённые по горизонтали.
//!
//! Idle-анимация процедурная, по ролям частей и их шарнирам (точки привязки
//! символов — оси, вокруг которых части крутили аниматоры DHX): дыхание
//! (всё, кроме ног, чуть поднимается), шея наклоняется, голова с гривой
//! качается вокруг шарнира головы (и следует за шеей), хвост покачивается,
//! крылья чуть "дышат". Скелет — цепочка ролей: торс -> шея -> голова/грива.

// Раны (`Wound`, `draw_wounds`) нужны restroom, а не просмотрщику `ponies`.
#![allow(dead_code)]

use std::cell::Cell;

use alkash3d_rs::engine::{AlkashEngine, Mesh, MeshInstance, Vertex};
use alkash3d_rs::math::{EulerRot, Quat, Vec2, Vec3};

/// Роль части в пакете: тень флэш-паппета не рисуем (своя тень-пятно).
const ROLE_SHADOW: u32 = 9;
pub const ROLE_NECK: u32 = 2;
pub const ROLE_HEAD: u32 = 3;
pub const ROLE_MANE: u32 = 4;
pub const ROLE_FRONT_LEG: u32 = 5;
pub const ROLE_HIND_LEG: u32 = 6;
pub const ROLE_TAIL: u32 = 7;
pub const ROLE_WING: u32 = 8;
/// Шаг глубины между соседними частями (м).
const LAYER_STEP: f32 = 0.0006;

/// Черты лица в пакете v2 (голова разобрана на слои): часть-черта хранит все
/// кадры своего символа DHX, кадр выбирается по `Expression`.
pub const FEATURE_BLINK: u32 = 1;
pub const FEATURE_BROW: u32 = 2;
pub const FEATURE_MOUTH: u32 = 3;
pub const FEATURE_NOSTRIL: u32 = 4;
pub const FEATURE_BLUSH: u32 = 5;
/// Ноги с позами DHX (кадры символа ноги: прямая, согнутая, поднятая...).
pub const LEG_FRONT: u32 = 6;
pub const LEG_HIND: u32 = 7;
/// Анимация по времени: текущая грива/хвост аликорнов, свечение магии рога (24 кадра/с).
pub const ANIM: u32 = 10;
const NONE_TEX: u32 = u32::MAX;
/// Сдвиг угла ракурсов варианта B в пакете (export_official.py VARIANT_B).
const VARIANT_B: f32 = 10.0;
const NO_MESH: usize = usize::MAX;

/// Форма глаз (блоки кадров символов Blink_* у DHX: normal 0, half lid 10,
/// squinty 20, cheerie 30, sad 50; внутри блока — моргание).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Eyes {
    #[default]
    Normal,
    HalfLid,
    Squinty,
    Cheerie,
    Sad,
}

impl Eyes {
    pub const ALL: [Eyes; 5] = [Eyes::Normal, Eyes::HalfLid, Eyes::Squinty, Eyes::Cheerie, Eyes::Sad];
    fn frame(self) -> u32 {
        match self {
            Eyes::Normal => 0,
            Eyes::HalfLid => 10,
            Eyes::Squinty => 20,
            Eyes::Cheerie => 30,
            Eyes::Sad => 50,
        }
    }
}

/// Настроение рта (блоки Mouth_*/Nostrilsync_*: happy 0, sad 40).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mood {
    #[default]
    Happy,
    Sad,
}

/// Фонема липсинка (метки внутри блока рта DHX).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phoneme {
    /// Рот закрыт (первый кадр блока).
    #[default]
    Rest,
    Aei,
    Rou,
    S,
    L,
    F,
    Th,
    Mbp,
}

impl Phoneme {
    pub const ALL: [Phoneme; 8] = [Phoneme::Rest, Phoneme::Aei, Phoneme::Rou, Phoneme::S, Phoneme::L, Phoneme::F, Phoneme::Th, Phoneme::Mbp];
    fn frame(self) -> u32 {
        match self {
            Phoneme::Rest => 0,
            Phoneme::Aei => 4,
            Phoneme::Rou => 11,
            Phoneme::S => 14,
            Phoneme::L => 18,
            Phoneme::F => 21,
            Phoneme::Th => 23,
            Phoneme::Mbp => 25,
        }
    }
}

/// Эмоция и липсинк лица — работают в любом ракурсе (у каждого ракурса свои
/// символы черт DHX с одинаковой раскладкой кадров).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Expression {
    pub eyes: Eyes,
    /// Фаза моргания внутри блока глаз: 0 открыт, 1 полу, 2–3 закрыт, 4 полу.
    pub blink: u32,
    pub mood: Mood,
    pub phoneme: Phoneme,
    /// Добавка к кадру фонемы (раскрытие рта внутри метки), обычно 0.
    pub open: u32,
    /// Кадр символа брови; None — бровей нет (как в нейтральной позе DHX).
    /// Брови у DHX нарисованы только от анфаса до ~80°.
    pub brow: Option<u32>,
    /// Кадр румянца; None — нет.
    pub blush: Option<u32>,
}

impl Expression {
    /// Кадр символа черты вида `kind`; None — черту не рисовать.
    fn frame(&self, kind: u32) -> Option<u32> {
        match kind {
            // С бровями у DHX — веки без верхних ресниц (блоки +100), иначе ресницы лезут на брови.
            // Первый кадр блока — открытый глаз без века; форма (half lid, squinty...)
            // держится со второго, моргание идёт дальше по блоку (2–3 закрыт, 4 полу).
            FEATURE_BLINK => {
                let rest = if self.eyes == Eyes::Normal { 0 } else { 1 };
                Some(self.eyes.frame() + self.blink.min(9).max(rest) + if self.brow.is_some() { 100 } else { 0 })
            }
            FEATURE_MOUTH | FEATURE_NOSTRIL => Some(match self.mood { Mood::Happy => 0, Mood::Sad => 40 } + self.phoneme.frame() + self.open),
            FEATURE_BROW => self.brow,
            FEATURE_BLUSH => self.blush,
            _ => Some(0),
        }
    }
}

/// Автоморгание: раз в ~4 с (своя фаза у каждого) — фазы 1-2-3-4 за 0.2 с.
pub fn auto_blink(t: f32, seed: f32) -> u32 {
    let period = 3.6 + (seed * 1.37).rem_euclid(1.5);
    let k = (t + seed * 7.0).rem_euclid(period);
    if k < 0.2 { 1 + (k / 0.05) as u32 } else { 0 }
}

struct Part {
    /// Основная картинка (у черты — кадр из файла; может не быть).
    mesh: Option<usize>,
    /// Отражённый (только у пол-оборота).
    mirrored: Option<usize>,
    /// Вид черты лица (0 — обычная часть) и её кадры (картинка; None — пусто).
    kind: u32,
    states: Vec<Option<Img>>,
    /// Меши кадров черты [прямой, отражённый], создаются при первом показе (`draw_pose_ex`).
    cache: Vec<[Cell<usize>; 2]>,
    /// Ноги: высота копыта (низ картинки, м) у каждой позы; NaN — поза пустая.
    reach: Vec<f32>,
    role: u32,
    /// Шарнир (точка привязки символа) в метрах плоскости персонажа.
    pivot: Vec2,
    /// Габариты картинки части в плоскости персонажа (для ран и подтёков).
    lo: Vec2,
    hi: Vec2,
    /// Сдвиг по глубине (порядок слоёв).
    z: f32,
    /// Номер среди частей той же роли в ракурсе (ближняя/дальняя нога).
    slot: u32,
}

/// Рана на теле (см. `Puppet::draw_wounds`): привязана к роли части и к
/// точке внутри её габаритов (0..1), поэтому держится на месте в любом
/// ракурсе и двигается вместе с частью.
#[derive(Clone, Copy)]
pub struct Wound {
    pub role: u32,
    pub uv: Vec2,
    /// Размер пятна (м).
    pub size: f32,
    /// Квад раны (центр в начале координат, 1×1, плоскость XY).
    pub mesh: usize,
    /// Подтёк вниз: квад (верх в начале координат, длина 1 вниз) и длина (м).
    pub drip: Option<(usize, f32)>,
    pub angle: f32,
}

/// Поза персонажа на этот кадр: ракурс и повороты частей (idle).
pub struct Pose {
    view: usize,
    mirror: bool,
    face: Quat,
    neck: Turn2,
    head: Turn2,
    tail_a: f32,
    wing_a: f32,
    bob: Vec2,
    /// Лицо (пакет v2): по умолчанию нейтральное с автоморганием; менять перед `draw_pose`.
    pub expr: Expression,
    /// Шаг на месте: фаза цикла (рад); None — стоит. Ноги качаются вокруг
    /// своих шарниров рысью (диагональные пары вместе).
    pub walk: Option<f32>,
    /// Добавка игры к повороту хвоста и гривы (рад, в сторону "назад от морды"):
    /// физика раскачки (см. play.rs) — сюда её результат.
    pub tail_extra: f32,
    pub mane_extra: f32,
    /// Взмах крыльев (рад, вверх от покоя) — игра машет ими в полёте.
    pub wing_extra: f32,
    /// Не рисовать крылья (пони «без крыльев»).
    pub no_wings: bool,
    /// Время для анимированных частей (с).
    pub anim_t: f32,
    /// Насколько ракурс боковой (|sin угла|): в анфас ноги не машут, а поднимаются.
    side: f32,
    height: f32,
}

struct View {
    /// Угол ракурса (рад): 0 — анфас, π/2 — мордой влево на экране.
    angle: f32,
    /// Вариант B (магия рога / раскрытые крылья): в пакете угол + VARIANT_B.
    variant: bool,
    parts: Vec<Part>,
    /// Шарниры шеи и головы этого ракурса (первые части с такой ролью).
    neck: Option<Vec2>,
    head: Option<Vec2>,
}

/// Поворот в плоскости персонажа вокруг точки: p -> R(a) p + o.
#[derive(Clone, Copy)]
struct Turn2 {
    a: f32,
    o: Vec2,
}

impl Turn2 {
    const NONE: Turn2 = Turn2 { a: 0.0, o: Vec2::ZERO };

    fn around(c: Vec2, a: f32) -> Turn2 {
        Turn2 { a, o: c - Vec2::from_angle(a).rotate(c) }
    }

    /// Сначала `self`, потом `then`.
    fn then(self, then: Turn2) -> Turn2 {
        Turn2 { a: self.a + then.a, o: Vec2::from_angle(then.a).rotate(self.o) + then.o }
    }
}

pub struct Puppet {
    pub name: String,
    pub height: f32,
    pub width: f32,
    views: Vec<View>,
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
    path: &'a str,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let s = self.data.get(self.at..self.at + n).ok_or_else(|| format!("{}: файл обрывается", self.path))?;
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> Result<u32, String> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn f32(&mut self) -> Result<f32, String> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn string(&mut self) -> Result<String, String> {
        let n = self.u32()? as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
}

/// Квад части: углы текстуры через матрицу пакета, нормаль +Z; `mirror` — отражение по X.
fn part_quad(w: u32, h: u32, m: [f32; 6], z: f32, tex: u32, mirror: bool) -> Result<Mesh, String> {
    let [a, b, c, d, e, f] = m;
    let sx = if mirror { -1.0 } else { 1.0 };
    let corner = |u: f32, v: f32| (sx * (a * u + c * v + e), b * u + d * v + f);
    let vert = |u: f32, v: f32| {
        let (x, y) = corner(u, v);
        Vertex::with_normal_uv(x, y, z, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 1.0, u / w as f32, v / h as f32)
    };
    let (w, h) = (w as f32, h as f32);
    let verts = [vert(0.0, h), vert(w, h), vert(w, 0.0), vert(0.0, 0.0)];
    // Матрица может отражать (отрицательный определитель) — тогда обход обратный,
    // иначе лицевая сторона уйдёт от камеры.
    let idx: &[u32] = if (a * d - b * c) * sx >= 0.0 { &[0, 1, 2, 0, 2, 3] } else { &[0, 2, 1, 0, 3, 2] };
    let mut mesh = Mesh::from_vertices_and_indices(&verts, idx).map_err(|e| format!("{:?}", e))?;
    mesh.albedo_srv_index = Some(tex);
    mesh.alpha_cutoff = 0.5;
    mesh.unlit = true;
    Ok(mesh)
}

/// Картинка части: текстура, размер, матрица пакета.
#[derive(Clone, Copy)]
struct Img {
    tex: u32,
    w: u32,
    h: u32,
    m: [f32; 6],
}

/// Часть в пакете до создания мешей.
struct RawPart {
    base: Option<Img>,
    z: f32,
    role: u32,
    pivot: Vec2,
    kind: u32,
    states: Vec<Option<Img>>,
}

/// Текстура пакета на GPU (создаётся при первом обращении).
fn pack_texture(engine: &mut AlkashEngine, data: &[u8], tex_at: &[(usize, u32, u32)], textures: &mut [Option<u32>], ti: u32, m: [f32; 6], path: &str) -> Result<Option<Img>, String> {
    if ti == NONE_TEX {
        return Ok(None);
    }
    let ti = ti as usize;
    let &(at, w, h) = tex_at.get(ti).ok_or_else(|| format!("{}: нет текстуры {}", path, ti))?;
    let tex = match textures[ti] {
        Some(tex) => tex,
        None => {
            // Загрузка сотен текстур — секунды; окно должно отвечать.
            engine.process_messages();
            let tex = engine.create_texture_rgba(w, h, &data[at..at + (w * h * 4) as usize]).ok_or("текстура части не создалась")?;
            textures[ti] = Some(tex);
            tex
        }
    };
    Ok(Some(Img { tex, w, h, m }))
}

fn read_matrix(r: &mut Reader) -> Result<[f32; 6], String> {
    let mut m = [0.0f32; 6];
    for v in m.iter_mut() {
        *v = r.f32()?;
    }
    Ok(m)
}

/// `only` — загрузить только этих персонажей (и только их текстуры);
/// None — всех. Пакеты версий 1 и 2 (2 — голова слоями, черты лица с кадрами).
pub fn read_pack(engine: &mut AlkashEngine, path: &str, only: Option<&[&str]>) -> Result<(f32, Vec<Puppet>), String> {
    let data = std::fs::read(path).map_err(|e| format!("не удалось прочитать {}: {}", path, e))?;
    let mut r = Reader { data: &data, at: 0, path };
    if r.take(4)? != b"MLPO" {
        return Err(format!("{}: не MLPO", path));
    }
    let version = r.u32()?;
    if version != 1 && version != 2 {
        return Err(format!("{}: версия {} не поддерживается", path, version));
    }
    let tint = r.f32()?;
    // Текстуры идут до персонажей: сначала только запомнить, где лежат, —
    // на GPU уйдут лишь те, что нужны выбранным персонажам.
    let ntex = r.u32()? as usize;
    let mut tex_at = Vec::with_capacity(ntex);
    for _ in 0..ntex {
        let (w, h) = (r.u32()?, r.u32()?);
        tex_at.push((r.at, w, h));
        r.take((w * h * 4) as usize)?;
    }
    let mut textures: Vec<Option<u32>> = vec![None; ntex];
    let nchars = r.u32()? as usize;
    let mut out = Vec::with_capacity(nchars);
    for _ in 0..nchars {
        let name = r.string()?;
        let height = r.f32()?;
        let nviews = r.u32()? as usize;
        let wanted = only.is_none_or(|o| o.contains(&name.as_str()));
        let mut raw: Vec<(f32, Vec<RawPart>)> = Vec::with_capacity(nviews);
        for _ in 0..nviews {
            let _frame = r.u32()?;
            let angle = r.f32()?;
            let nparts = r.u32()? as usize;
            let mut parts = Vec::with_capacity(nparts);
            for k in 0..nparts {
                let ti = r.u32()?;
                let role = r.u32()?;
                let _symbol = r.string()?;
                let m = read_matrix(&mut r)?;
                let pivot = Vec2::new(r.f32()?, r.f32()?);
                let (kind, nstates) = if version >= 2 { (r.u32()?, r.u32()? as usize) } else { (0, 0) };
                let mut st = Vec::with_capacity(nstates);
                for _ in 0..nstates {
                    let sti = r.u32()?;
                    st.push((sti, read_matrix(&mut r)?));
                }
                if role == ROLE_SHADOW || !wanted {
                    continue;
                }
                let base = pack_texture(engine, &data, &tex_at, &mut textures, ti, m, path)?;
                let mut states = Vec::with_capacity(st.len());
                for (sti, sm) in st {
                    states.push(pack_texture(engine, &data, &tex_at, &mut textures, sti, sm, path)?);
                }
                parts.push(RawPart { base, z: k as f32 * LAYER_STEP, role, pivot, kind, states });
            }
            raw.push((angle, parts));
        }
        if !wanted {
            continue;
        }
        // Ширина для расстановки — по основной позе (первый ракурс): в
        // повороте грива/крылья местами разлетаются далеко в стороны.
        // Ширина фигуры — по самому дальнему от центра углу части (X = a u + c v + e).
        let mut width = 0.0f32;
        if let Some((_, parts)) = raw.first() {
            for img in parts.iter().filter_map(|p| p.base) {
                let [a, _, c, _, e, _] = img.m;
                for (u, v) in [(0.0, 0.0), (img.w as f32, img.h as f32), (img.w as f32, 0.0), (0.0, img.h as f32)] {
                    width = width.max(2.0 * (a * u + c * v + e).abs());
                }
            }
        }
        let half = raw.iter().filter(|(a, _)| *a < VARIANT_B * 0.5).all(|(a, _)| *a <= std::f32::consts::PI + 1e-3);
        let mut views = Vec::with_capacity(raw.len());
        for (angle, parts) in raw {
            let mut out = Vec::with_capacity(parts.len());
            for p in parts {
                let (mut mesh, mut mirrored) = (None, None);
                let (mut lo, mut hi) = (p.pivot, p.pivot);
                if let Some(img) = p.base {
                    mesh = Some(engine.add_mesh(part_quad(img.w, img.h, img.m, p.z, img.tex, false)?));
                    if half {
                        mirrored = Some(engine.add_mesh(part_quad(img.w, img.h, img.m, p.z, img.tex, true)?));
                    }
                    let [a, b, c, d, e, f] = img.m;
                    let (wf, hf) = (img.w as f32, img.h as f32);
                    (lo, hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
                    for (u, v) in [(0.0, 0.0), (wf, hf), (wf, 0.0), (0.0, hf)] {
                        let q = Vec2::new(a * u + c * v + e, b * u + d * v + f);
                        lo = lo.min(q);
                        hi = hi.max(q);
                    }
                }
                let cache = (0..p.states.len()).map(|_| [Cell::new(NO_MESH), Cell::new(NO_MESH)]).collect();
                let slot = out.iter().filter(|q: &&Part| q.role == p.role).count() as u32;
                let reach = p.states.iter().map(|st| st.map_or(f32::NAN, |img| {
                    let [_, b, _, d, _, f] = img.m;
                    let (wf, hf) = (img.w as f32, img.h as f32);
                    [(0.0, 0.0), (wf, 0.0), (0.0, hf), (wf, hf)].iter().map(|&(u, v)| b * u + d * v + f).fold(f32::MAX, f32::min)
                })).collect();
                out.push(Part { mesh, mirrored, kind: p.kind, states: p.states, cache, reach, role: p.role, pivot: p.pivot, lo, hi, z: p.z, slot });
            }
            let first = |r: u32| out.iter().find(|p| p.role == r).map(|p| p.pivot);
            let (neck, head) = (first(ROLE_NECK), first(ROLE_HEAD));
            let variant = angle >= VARIANT_B * 0.5;
            let angle = if variant { angle - VARIANT_B } else { angle };
            views.push(View { angle, variant, parts: out, neck, head });
        }
        out.push(Puppet { name, height, width, views });
    }
    Ok((tint, out))
}

impl Puppet {
    /// Ракурс и idle-повороты для персонажа, смотрящего по `facing`, с
    /// камерой в `cam`; `t` — время (с), `seed` — своя фаза.
    pub fn pose(&self, at: Vec3, facing: Vec3, cam: Vec3, t: f32, seed: f32) -> Option<Pose> {
        self.pose_v(at, facing, cam, t, seed, false)
    }

    /// Есть ли у персонажа крылья (пегасы, аликорны).
    pub fn has_wings(&self) -> bool {
        self.views.iter().any(|v| v.parts.iter().any(|p| p.role == ROLE_WING))
    }

    /// Есть ли у персонажа вариант B (магия рога или раскрытые крылья — по пони).
    pub fn has_variant_b(&self) -> bool {
        self.views.iter().any(|v| v.variant)
    }

    /// Как `pose`, но с выбором варианта: `b` — ракурсы варианта B (если они есть).
    pub fn pose_v(&self, at: Vec3, facing: Vec3, cam: Vec3, t: f32, seed: f32, b: bool) -> Option<Pose> {
        let mut c = cam - at;
        c.y = 0.0;
        let c = c.normalize_or_zero();
        // Угол ракурса: facing = cos θ · c + sin θ · (c × Y) — при θ = π/2
        // персонаж на экране смотрит влево.
        let side = c.cross(Vec3::Y);
        let mut theta = facing.dot(side).atan2(facing.dot(c));
        // Пол-оборота: другой бок — зеркало ракурса с углом -θ.
        let mirror = theta < 0.0 && self.views.iter().any(|v| v.parts.iter().any(|p| p.mirrored.is_some()));
        if mirror {
            theta = -theta;
        }
        let dist = |a: f32| {
            let d = (a - theta).rem_euclid(std::f32::consts::TAU);
            d.min(std::f32::consts::TAU - d)
        };
        // Основная поза (кадр 0) — только если ближе всех; в повороте ракурсы равномерны.
        let want_b = b && self.has_variant_b();
        let view = (0..self.views.len()).filter(|&i| self.views[i].variant == want_b).min_by(|&a, &b| dist(self.views[a].angle).total_cmp(&dist(self.views[b].angle)))?;
        let v = &self.views[view];
        let face = Quat::from_rotation_y(c.x.atan2(c.z));

        // Idle: дыхание ~0.3 Гц, голова и хвост — на своих частотах.
        // Отладка: PONIES_IDLE_AMP — множитель размаха (видно, верны ли шарниры).
        static AMP: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
        let amp = *AMP.get_or_init(|| std::env::var("PONIES_IDLE_AMP").ok().and_then(|v| v.parse().ok()).unwrap_or(1.0));
        let w = std::f32::consts::TAU;
        let breath = (w * 0.28 * t + seed).sin();
        // Отражённый ракурс: шарнир по X и угол — с обратным знаком.
        let sx = if mirror { -1.0 } else { 1.0 };
        let flip = |p: Vec2| Vec2::new(p.x * sx, p.y);
        let neck = v.neck.map_or(Turn2::NONE, |c| Turn2::around(flip(c), sx * amp * 0.025 * (w * 0.28 * t + seed - 0.6).sin()));
        let head = v.head.map_or(Turn2::NONE, |c| {
            let a = 0.04 * (w * 0.19 * t + seed * 1.7).sin() + 0.015 * breath;
            Turn2::around(flip(c), sx * amp * a)
        });
        Some(Pose {
            view,
            mirror,
            face,
            neck,
            head,
            tail_a: sx * amp * 0.07 * (w * 0.33 * t + seed * 2.3).sin(),
            wing_a: sx * amp * 0.03 * breath,
            bob: Vec2::new(0.0, 0.006 * self.height * breath * amp),
            expr: Expression { blink: auto_blink(t, seed), ..Expression::default() },
            walk: None,
            tail_extra: 0.0,
            mane_extra: 0.0,
            wing_extra: 0.0,
            no_wings: false,
            anim_t: t,
            side: v.angle.sin().abs(),
            height: self.height,
        })
    }

    /// Нога в шаге: (угол вокруг шарнира, подъём копыта). Рысь: передняя 0 с задней 1,
    /// передняя 1 с задней 0.
    fn leg_swing(pose: &Pose, role: u32, slot: u32) -> (f32, f32) {
        let Some(s) = Self::leg_phase(pose, role, slot) else { return (0.0, 0.0) };
        let sx = if pose.mirror { -1.0 } else { 1.0 };
        // Вперёд по экрану — к морде: у ракурса без отражения морда слева (−X).
        (sx * 0.32 * pose.side * s, 0.035 * pose.height * s.max(0.0) * (1.0 - 0.6 * pose.side))
    }

    /// Фаза ноги в шаге −1..1 (>0 — перенос, нога в воздухе); None — стоит.
    fn leg_phase(pose: &Pose, role: u32, slot: u32) -> Option<f32> {
        let ph = pose.walk?;
        let pair = (role == ROLE_FRONT_LEG) == (slot % 2 == 0);
        Some((ph + if pair { 0.0 } else { std::f32::consts::PI }).sin())
    }

    /// Поза ноги в шаге: та, у которой копыто ближе всего к нужной высоте подъёма.
    fn leg_state(p: &Part, pose: &Pose) -> Option<usize> {
        let s = Self::leg_phase(pose, p.role, p.slot)?;
        let lo = p.reach.iter().copied().filter(|r| r.is_finite()).fold(f32::MAX, f32::min);
        let hi = p.reach.iter().copied().filter(|r| r.is_finite()).fold(f32::MIN, f32::max);
        if !(hi > lo) {
            return None;
        }
        let want = lo + (hi - lo) * s.max(0.0);
        (0..p.reach.len()).filter(|&i| p.reach[i].is_finite()).min_by(|&a, &b| (p.reach[a] - want).abs().total_cmp(&(p.reach[b] - want).abs()))
    }

    /// Поворот части с ролью `role` (шарнир `pivot` уже отражён) и подъём от дыхания.
    fn part_turn(pose: &Pose, role: u32, slot: u32, pivot: Vec2, posed: bool) -> (Turn2, Vec2) {
        let (mut swing, mut raise) = Self::leg_swing(pose, role, slot);
        if posed {
            // Подъём даёт сама поза DHX; поворот — только чуть, для шага вперёд-назад.
            swing *= 0.45;
            raise = 0.0;
        }
        let local = match role {
            ROLE_FRONT_LEG | ROLE_HIND_LEG => Turn2::around(pivot, swing),
            ROLE_NECK => pose.neck,
            ROLE_HEAD => pose.head.then(pose.neck),
            // Грива на шее: вместе с головой и шеей плюс своя раскачка вокруг шарнира.
            ROLE_MANE => Turn2::around(pivot, Self::sx(pose) * pose.mane_extra).then(pose.head).then(pose.neck),
            ROLE_TAIL => Turn2::around(pivot, pose.tail_a + Self::sx(pose) * pose.tail_extra),
            // Взмах: в профиль оба крыла вверх (назад от морды — против часовой у ракурса без
            // отражения), в анфас крылья расходятся в разные стороны.
            ROLE_WING => {
                let side = if pose.side < 0.5 && slot % 2 == 1 { -1.0 } else { 1.0 };
                Turn2::around(pivot, pose.wing_a - Self::sx(pose) * side * pose.wing_extra)
            }
            _ => Turn2::NONE,
        };
        // В шаге корпус подпрыгивает дважды за цикл.
        let step_bob = pose.walk.map_or(0.0, |ph| 0.012 * pose.height * (2.0 * ph).sin().abs());
        let lift = if matches!(role, ROLE_FRONT_LEG | ROLE_HIND_LEG) { Vec2::new(0.0, raise) } else { pose.bob + Vec2::new(0.0, step_bob) };
        (local, lift)
    }

    /// Знак "назад от морды" на экране: у ракурса без отражения морда слева.
    fn sx(pose: &Pose) -> f32 {
        if pose.mirror { -1.0 } else { 1.0 }
    }

    fn flip(pose: &Pose, p: Vec2) -> Vec2 {
        if pose.mirror { Vec2::new(-p.x, p.y) } else { p }
    }

    /// Ракурс для персонажа, смотрящего по `facing`, с камерой в `cam`.
    /// `t` — время (с), `seed` — своя фаза у каждого персонажа.
    /// `scale` — единиц сцены в метре (пакет — в метрах).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(&self, out: &mut Vec<MeshInstance>, at: Vec3, facing: Vec3, cam: Vec3, tint: f32, t: f32, seed: f32, scale: f32) {
        if let Some(pose) = self.pose(at, facing, cam, t, seed) {
            self.draw_pose(out, &pose, at, tint, scale);
        }
    }

    pub fn draw_pose(&self, out: &mut Vec<MeshInstance>, pose: &Pose, at: Vec3, tint: f32, scale: f32) {
        self.draw_parts(None, out, pose, at, tint, scale);
    }

    /// Как `draw_pose`, но с эмоцией `pose.expr` на чертах лица (пакет v2):
    /// меши нужных кадров черт создаются в движке при первом показе.
    pub fn draw_pose_ex(&self, engine: &mut AlkashEngine, out: &mut Vec<MeshInstance>, pose: &Pose, at: Vec3, tint: f32, scale: f32) {
        self.draw_parts(Some(engine), out, pose, at, tint, scale);
    }

    /// Меш части в этой позе; None — не рисовать (черта скрыта или кадр пуст).
    fn part_mesh(engine: &mut Option<&mut AlkashEngine>, p: &Part, pose: &Pose) -> Option<usize> {
        let base = if pose.mirror { p.mirrored.or(p.mesh) } else { p.mesh };
        if p.kind == 0 || p.states.is_empty() {
            return base;
        }
        let frame = if p.kind == ANIM {
            ((pose.anim_t.max(0.0) * 24.0) as usize) % p.states.len()
        } else if matches!(p.kind, LEG_FRONT | LEG_HIND) {
            match Self::leg_state(p, pose) {
                Some(i) => i,
                None => return base,
            }
        } else {
            pose.expr.frame(p.kind)? as usize
        };
        let i = frame.min(p.states.len() - 1);
        let img = p.states[i]?;
        let side = usize::from(pose.mirror);
        let cell = &p.cache[i][side];
        if cell.get() == NO_MESH {
            let Some(engine) = engine.as_deref_mut() else { return base };
            let quad = part_quad(img.w, img.h, img.m, p.z, img.tex, pose.mirror).ok()?;
            cell.set(engine.add_mesh(quad));
        }
        Some(cell.get())
    }

    fn draw_parts(&self, mut engine: Option<&mut AlkashEngine>, out: &mut Vec<MeshInstance>, pose: &Pose, at: Vec3, tint: f32, scale: f32) {
        for p in &self.views[pose.view].parts {
            if pose.no_wings && p.role == ROLE_WING {
                continue;
            }
            let Some(mesh) = Self::part_mesh(&mut engine, p, pose) else { continue };
            let posed = matches!(p.kind, LEG_FRONT | LEG_HIND) && p.reach.iter().filter(|r| r.is_finite()).count() > 1;
            let (local, lift) = Self::part_turn(pose, p.role, p.slot, Self::flip(pose, p.pivot), posed);
            let off = pose.face * ((local.o + lift) * scale).extend(0.0);
            let (rz, ry, rx) = (pose.face * Quat::from_rotation_z(local.a)).to_euler(EulerRot::ZYX);
            out.push(MeshInstance::new(mesh).at(at.x + off.x, at.y + off.y, at.z + off.z).rotated(rx, ry, rz).scaled(scale, scale, scale).tinted(tint, tint, tint, 1.0));
        }
    }

    /// Где рана в этом ракурсе: точка в плоскости персонажа (уже с поворотом
    /// части), поворот части, глубина поверх неё.
    fn wound_at(&self, pose: &Pose, w: &Wound) -> Option<(Vec2, f32, f32)> {
        let parts: Vec<&Part> = self.views[pose.view].parts.iter().filter(|p| p.role == w.role).collect();
        let first = parts.first()?;
        let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
        let mut z = 0.0f32;
        for p in &parts {
            lo = lo.min(p.lo);
            hi = hi.max(p.hi);
            z = z.max(p.z);
        }
        let q = Self::flip(pose, lo + (hi - lo) * w.uv);
        let (local, lift) = Self::part_turn(pose, w.role, first.slot, Self::flip(pose, first.pivot), false);
        let p = Vec2::from_angle(local.a).rotate(q) + local.o + lift;
        Some((p, local.a, z + LAYER_STEP * 0.5))
    }

    /// Точка раны в мире (откуда летят брызги).
    pub fn wound_world(&self, pose: &Pose, at: Vec3, w: &Wound, scale: f32) -> Option<Vec3> {
        let (p, _, z) = self.wound_at(pose, w)?;
        Some(at + pose.face * (Vec3::new(p.x, p.y, z) * scale))
    }

    /// Раны и подтёки поверх частей (без освещения, как сами части).
    pub fn draw_wounds(&self, out: &mut Vec<MeshInstance>, pose: &Pose, at: Vec3, wounds: &[Wound], tint: f32, scale: f32) {
        for w in wounds {
            let Some((p, a, z)) = self.wound_at(pose, w) else { continue };
            let pos = at + pose.face * (Vec3::new(p.x, p.y, z) * scale);
            let (rz, ry, rx) = (pose.face * Quat::from_rotation_z(a + w.angle)).to_euler(EulerRot::ZYX);
            let s = w.size * scale;
            out.push(MeshInstance::new(w.mesh).at(pos.x, pos.y, pos.z).rotated(rx, ry, rz).scaled(s, s, 1.0).tinted(tint, tint, tint, 1.0));
            if let Some((drip, len)) = w.drip.filter(|d| d.1 > 0.001) {
                // Подтёк — вниз по части (с её поворотом), чуть ближе к камере, чем рана.
                let pos = at + pose.face * (Vec3::new(p.x, p.y, z + LAYER_STEP * 0.25) * scale);
                let (rz, ry, rx) = (pose.face * Quat::from_rotation_z(a)).to_euler(EulerRot::ZYX);
                out.push(MeshInstance::new(drip).at(pos.x, pos.y, pos.z).rotated(rx, ry, rz).scaled(s * 0.35, len * scale, 1.0).tinted(tint, tint, tint, 1.0));
            }
        }
    }
}
