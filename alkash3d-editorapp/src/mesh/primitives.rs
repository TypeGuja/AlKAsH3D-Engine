use super::Mesh;
use crate::math::Vec3;

impl Mesh {
    pub fn create_cube() -> Self {
        // ОБНОВЛЕНО (честная UV-развёртка примитивов — по прямому запросу
        // пользователя): раньше куб состоял из 8 РАЗДЕЛЯЕМЫХ вершин на 6
        // граней (см. историю git) — у вершины на углу куба нормаль после
        // `recalculate_normals()` усредняется по ВСЕМ 3 сходящимся в нём
        // граням (например (0.577,0.577,0.577) в самом дальнем углу), и
        // `recalculate_uv()` выбирает доминирующую ось ИМЕННО по этой
        // усреднённой нормали — одну и ту же для всех граней, которым эта
        // вершина принадлежит. Итог: у всех 8 вершин куба UV фактически
        // схлопывался в 4 угла квадрата (0,0)/(0,1)/(1,0)/(1,1) независимо от
        // того, какой из 6 граней вершина реально принадлежит — во
        // "🗺 View UV Unwrap..." это было видно буквально как одна диагональ
        // через всю развёртку вместо честной сетки 6 квадов.
        //
        // Теперь у КАЖДОЙ из 6 граней куба — свои 4 СОБСТВЕННЫЕ вершины (24
        // всего, ни одна не используется больше чем одной гранью). После
        // `recalculate_normals()` у всех 4 вершин одной грани нормаль
        // получается ОДНА и та же чистая осевая нормаль (без усреднения с
        // соседями), поэтому `recalculate_uv()` у всех четырёх выбирает
        // одну и ту же доминирующую ось — угол больше не "срезает" развёртку.
        //
        // Побочный эффект для Edit Mode: вершины куба больше не общие между
        // гранями, поэтому `detach_faces_from_neighbors` на кубе теперь
        // no-op (нечего отрывать — грани и так уже независимы), а перемещение
        // одной вершины больше не тянет соседние грани вместе с ней. Для
        // сферы/цилиндра/тора (там общие вершины между соседними
        // треугольниками нужны ради ГЛАДКОГО шейдинга, не то же самое, что у
        // куба) топология не менялась — см. `mesh/uv.rs` про их отдельные
        // (более мягкие) UV-артефакты на швах.
        let p = [
            Vec3::new(-0.5, -0.5, -0.5), // 0
            Vec3::new(0.5, -0.5, -0.5),  // 1
            Vec3::new(0.5, 0.5, -0.5),   // 2
            Vec3::new(-0.5, 0.5, -0.5),  // 3
            Vec3::new(-0.5, -0.5, 0.5),  // 4
            Vec3::new(0.5, -0.5, 0.5),   // 5
            Vec3::new(0.5, 0.5, 0.5),    // 6
            Vec3::new(-0.5, 0.5, 0.5),   // 7
        ];
        // Порядок граней и навивка каждого квада (a,b,c,d) -> треугольники
        // (a,b,c)+(a,c,d) сохраняют ту же самую "наружу" ориентацию, что и
        // прежние индексы (см. комментарий про backface culling выше по
        // git-истории) — проверено (v1-v0)×(v2-v0) для каждой грани.
        let faces: [[usize; 4]; 6] = [
            [0, 3, 2, 1], // back   (z=-0.5, нормаль -z)
            [4, 5, 6, 7], // front  (z=+0.5, нормаль +z)
            [0, 4, 7, 3], // left   (x=-0.5, нормаль -x)
            [1, 2, 6, 5], // right  (x=+0.5, нормаль +x)
            [0, 1, 5, 4], // bottom (y=-0.5, нормаль -y)
            [3, 7, 6, 2], // top    (y=+0.5, нормаль +y)
        ];

        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);
        for quad in &faces {
            let base = vertices.len() as u32;
            for &corner in quad {
                vertices.push(p[corner]);
            }
            indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        Self::new(vertices, indices)
    }

    pub fn create_sphere() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        let rings = 16;

        for i in 0..=rings {
            let phi = std::f32::consts::PI * i as f32 / rings as f32;
            let y = -phi.cos() * 0.5;
            let r = phi.sin() * 0.5;
            for j in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / segments as f32;
                vertices.push(Vec3::new(r * theta.cos(), y, r * theta.sin()));
            }
        }

        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + 1;
                let c = (i + 1) * (segments + 1) + j;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
            }
        }

        Self::new(vertices, indices)
    }

    pub fn create_plane() -> Self {
        let vertices = vec![
            Vec3::new(-5.0, 0.0, -5.0), Vec3::new(5.0, 0.0, -5.0),
            Vec3::new(5.0, 0.0, 5.0), Vec3::new(-5.0, 0.0, 5.0),
        ];
        let indices = vec![0,1,2, 2,3,0];
        Self::new(vertices, indices)
    }

    pub fn create_cylinder() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;

        for i in 0..segments {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = angle.cos() * 0.5;
            let z = angle.sin() * 0.5;
            vertices.push(Vec3::new(x, -0.5, z));
            vertices.push(Vec3::new(x, 0.5, z));
        }

        for i in 0..segments {
            let next = (i + 1) % segments;
            let base = (i * 2) as u32;
            let next_base = (next * 2) as u32;
            indices.extend_from_slice(&[base, base+1, next_base, next_base, base+1, next_base+1]);
        }

        Self::new(vertices, indices)
    }

    pub fn create_cone() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;

        vertices.push(Vec3::new(0.0, 0.5, 0.0));
        for i in 0..segments {
            let angle = 2.0 * std::f32::consts::PI * i as f32 / segments as f32;
            let x = angle.cos() * 0.5;
            let z = angle.sin() * 0.5;
            vertices.push(Vec3::new(x, -0.5, z));
        }

        for i in 0..segments {
            let next = (i + 1) % segments;
            indices.extend_from_slice(&[0, (i+1) as u32, (next+1) as u32]);
        }

        Self::new(vertices, indices)
    }

    pub fn create_torus() -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let segments = 24;
        let rings = 16;
        let r1 = 0.2;
        let r2 = 0.5;

        for i in 0..=rings {
            let phi = 2.0 * std::f32::consts::PI * i as f32 / rings as f32;
            for j in 0..=segments {
                let theta = 2.0 * std::f32::consts::PI * j as f32 / segments as f32;
                let x = (r2 + r1 * theta.cos()) * phi.cos();
                let y = r1 * theta.sin();
                let z = (r2 + r1 * theta.cos()) * phi.sin();
                vertices.push(Vec3::new(x, y, z));
            }
        }

        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = a + 1;
                let c = (i + 1) * (segments + 1) + j;
                let d = c + 1;
                indices.extend_from_slice(&[a as u32, b as u32, c as u32, b as u32, d as u32, c as u32]);
            }
        }

        Self::new(vertices, indices)
    }
}