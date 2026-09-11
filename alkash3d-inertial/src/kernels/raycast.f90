! inertial/src/kernels/raycast.f90
!! ДОБАВЛЕНО (полноценная физика — запрос луча против сцены): до этого
!! момента у ABI плагина не было НИКАКОГО способа спросить "что первым
!! встретится по этому лучу" — игровой код (например подвеска машины,
!! `alkash3d-rust/src/car_physics.rs`) был вынужден имитировать землю как
!! жёстко зашитую константную высоту `ground_y`, полностью игнорируя
!! реальную физическую геометрию (рампы, ямы, другие тела). Этот модуль —
!! честный raycast против ВСЕХ тел сцены (сфер и коробок, с учётом их
!! текущей ориентации) и статичных полуплоскостей (`plane_c`-подобных
!! данных, переданных как плоские массивы — см. `raycast_query` ниже),
!! возвращающий БЛИЖАЙШЕЕ пересечение.
!!
!! Формы: сфера — классическое аналитическое решение квадратного
!! уравнения; коробка — slab-тест в ЛОКАЛЬНОМ пространстве тела (луч
!! разворачивается обратным кватернионом ориентации, тест — как для
!! обычного AABB, найденная точка/нормаль разворачиваются обратно в
!! мировые координаты); плоскость — пересечение луча с бесконечной
!! плоскостью, нормаль возвращается РАЗВёРНУТОЙ НАВСТРЕЧУ лучу (тот же
!! принцип, что у большинства физдвижков — раскаст не обязан знать, с
!! какой стороны подошли).
module raycast_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c
    implicit none

    real(c_float), parameter :: RC_EPS = 1.0e-8

contains
    ! ===================================================================
    ! ВЕКТОРНЫЕ ХЕЛПЕРЫ
    ! ===================================================================
    pure function dot3(a, b) result(d)
        real(c_float), intent(in) :: a(3), b(3)
        real(c_float) :: d
        d = a(1)*b(1) + a(2)*b(2) + a(3)*b(3)
    end function dot3

    ! Поворот вектора `v` единичным кватернионом `q` (x,y,z,w) — эффективная
    ! формула без построения полной матрицы поворота: v' = v + 2*w*(qv×v) +
    ! 2*(qv×(qv×v)), где qv — векторная часть кватерниона.
    pure function rotate_vec_by_quat(v, q) result(vr)
        real(c_float), intent(in) :: v(3), q(4)
        real(c_float) :: vr(3), qv(3), t(3)
        qv = q(1:3)
        t(1) = 2.0 * (qv(2)*v(3) - qv(3)*v(2))
        t(2) = 2.0 * (qv(3)*v(1) - qv(1)*v(3))
        t(3) = 2.0 * (qv(1)*v(2) - qv(2)*v(1))
        vr(1) = v(1) + q(4)*t(1) + (qv(2)*t(3) - qv(3)*t(2))
        vr(2) = v(2) + q(4)*t(2) + (qv(3)*t(1) - qv(1)*t(3))
        vr(3) = v(3) + q(4)*t(3) + (qv(1)*t(2) - qv(2)*t(1))
    end function rotate_vec_by_quat

    ! Сопряжённый (= обратный для ЕДИНИЧНОГО) кватернион — переводит вектор
    ! из мировых координат в локальные оси тела.
    pure function quat_conjugate(q) result(qc)
        real(c_float), intent(in) :: q(4)
        real(c_float) :: qc(4)
        qc = [-q(1), -q(2), -q(3), q(4)]
    end function quat_conjugate

    ! ===================================================================
    ! ЛУЧ vs СФЕРА
    ! ===================================================================
    subroutine ray_sphere(origin, dir, center, radius, max_dist, hit, t, point, normal)
        real(c_float), intent(in) :: origin(3), dir(3), center(3), radius, max_dist
        integer(c_int), intent(out) :: hit
        real(c_float), intent(out) :: t, point(3), normal(3)
        real(c_float) :: oc(3), b, c, disc, sqrt_disc, t0, t1

        hit = 0
        t = 0.0
        point = 0.0
        normal = 0.0

        oc = origin - center
        b = dot3(oc, dir)
        c = dot3(oc, oc) - radius * radius
        disc = b*b - c
        if (disc < 0.0) return

        sqrt_disc = sqrt(disc)
        t0 = -b - sqrt_disc
        t1 = -b + sqrt_disc
        if (t0 >= 0.0) then
            t = t0
        else if (t1 >= 0.0) then
            t = t1 ! начало луча уже внутри сферы — ближайшее пересечение "снаружи"
        else
            return ! сфера целиком позади луча
        end if
        if (t > max_dist) return

        hit = 1
        point = origin + dir * t
        normal = (point - center) / max(radius, RC_EPS)
    end subroutine ray_sphere

    ! ===================================================================
    ! ЛУЧ vs КОРОБКА (OBB через локальное пространство тела)
    ! ===================================================================
    subroutine ray_box(origin, dir, center, orientation, half_extents, max_dist, hit, t, point, normal)
        real(c_float), intent(in) :: origin(3), dir(3), center(3), orientation(4), half_extents(3), max_dist
        integer(c_int), intent(out) :: hit
        real(c_float), intent(out) :: t, point(3), normal(3)

        real(c_float) :: inv_q(4), local_origin(3), local_dir(3), local_normal(3)
        real(c_float) :: t_near, t_far, inv_d, t1, t2, tmp
        integer :: axis
        real(c_float) :: axis_normal_sign

        hit = 0
        t = 0.0
        point = 0.0
        normal = 0.0

        inv_q = quat_conjugate(orientation)
        local_origin = rotate_vec_by_quat(origin - center, inv_q)
        local_dir = rotate_vec_by_quat(dir, inv_q)

        t_near = 0.0
        t_far = max_dist
        local_normal = [0.0, 0.0, 0.0]

        do axis = 1, 3
            if (abs(local_dir(axis)) < RC_EPS) then
                ! Луч параллелен этой паре граней — либо целиком внутри
                ! "слоя" по этой оси, либо мимо коробки навсегда.
                if (local_origin(axis) < -half_extents(axis) .or. local_origin(axis) > half_extents(axis)) then
                    return
                end if
                cycle
            end if

            inv_d = 1.0 / local_dir(axis)
            t1 = (-half_extents(axis) - local_origin(axis)) * inv_d
            t2 = (half_extents(axis) - local_origin(axis)) * inv_d
            axis_normal_sign = -1.0 ! входим через грань "-half_extents"
            if (t1 > t2) then
                tmp = t1; t1 = t2; t2 = tmp
                axis_normal_sign = 1.0 ! луч идёт в обратную сторону — входим через "+half_extents"
            end if

            if (t1 > t_near) then
                t_near = t1
                local_normal = [0.0, 0.0, 0.0]
                local_normal(axis) = axis_normal_sign
            end if
            t_far = min(t_far, t2)
            if (t_near > t_far) return
        end do

        if (t_near > max_dist) return

        hit = 1
        t = t_near
        point = center + rotate_vec_by_quat(local_origin + local_dir * t_near, orientation)
        normal = rotate_vec_by_quat(local_normal, orientation)
    end subroutine ray_box

    ! ===================================================================
    ! ЛУЧ vs ПОЛУПРОСТРАНСТВЕННАЯ ПЛОСКОСТЬ (пол и т.п., см. PlaneDesc)
    ! ===================================================================
    subroutine ray_plane(origin, dir, plane_normal, plane_point, max_dist, hit, t, point, normal)
        real(c_float), intent(in) :: origin(3), dir(3), plane_normal(3), plane_point(3), max_dist
        integer(c_int), intent(out) :: hit
        real(c_float), intent(out) :: t, point(3), normal(3)
        real(c_float) :: denom

        hit = 0
        t = 0.0
        point = 0.0
        normal = 0.0

        denom = dot3(dir, plane_normal)
        if (abs(denom) < RC_EPS) return ! луч параллелен плоскости

        t = dot3(plane_point - origin, plane_normal) / denom
        if (t < 0.0 .or. t > max_dist) return

        hit = 1
        point = origin + dir * t
        ! Нормаль всегда развёрнута НАВСТРЕЧУ лучу — тот же принцип, что у
        ! большинства физдвижков (raycast не обязан знать заранее, с какой
        ! стороны подошли).
        if (denom < 0.0) then
            normal = plane_normal
        else
            normal = -plane_normal
        end if
    end subroutine ray_plane

    ! ===================================================================
    ! ГЛАВНЫЙ ЗАПРОС: ближайшее пересечение среди всех тел + плоскостей
    ! ===================================================================
    ! `direction` ожидается НОРМИРОВАННЫМ вызывающей (Rust) стороной — так
    ! `hit_distance` честно является метрической длиной вдоль луча, а не
    ! долей от произвольного вектора. `plane_normals`/`plane_points` —
    ! плоские массивы 3×n_planes (см. вызов на Rust-стороне,
    ! `PhysicsState::raycast` в lib.rs) — компактнее отдельного bind(c)-типа
    ! на пару float(3), а слой между ABI-плоскостью движка (`PlaneDesc`) и
    ! этим вызовом и так уже существует в Rust (`PlaneRecord`).
    ! ДОБАВЛЕНО: `exclude_index` — 0-based индекс тела, которое НУЖНО
    ! пропустить (например собственный кузов машины при raycast'е подвески
    ! вниз, иначе луч почти всегда сразу упирается в свой же box-коллайдер,
    ! т.к. точка крепления колеса лежит на его границе или внутри него) —
    ! `-1` означает "не исключать никого".
    subroutine raycast_query(bodies, n_bodies, plane_normals, plane_points, n_planes, &
            origin, direction, max_dist, exclude_index, &
            hit_found, hit_distance, hit_point, hit_normal, hit_index, hit_is_plane) &
            bind(c, name="raycast_query")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(*)
        integer(c_int), intent(in), value :: n_bodies
        real(c_float), intent(in) :: plane_normals(3, *)
        real(c_float), intent(in) :: plane_points(3, *)
        integer(c_int), intent(in), value :: n_planes
        real(c_float), intent(in) :: origin(3)
        real(c_float), intent(in) :: direction(3)
        real(c_float), intent(in), value :: max_dist
        integer(c_int), intent(in), value :: exclude_index
        integer(c_int), intent(out) :: hit_found
        real(c_float), intent(out) :: hit_distance
        real(c_float), intent(out) :: hit_point(3)
        real(c_float), intent(out) :: hit_normal(3)
        integer(c_int), intent(out) :: hit_index
        integer(c_int), intent(out) :: hit_is_plane

        integer :: i, cand_hit
        real(c_float) :: cand_t, cand_point(3), cand_normal(3)
        real(c_float) :: best_t

        hit_found = 0
        hit_distance = max_dist
        hit_point = 0.0
        hit_normal = 0.0
        hit_index = -1
        hit_is_plane = 0
        best_t = max_dist

        do i = 1, n_bodies
            if (exclude_index >= 0 .and. (i - 1) == exclude_index) cycle
            if (bodies(i)%shape_type == 1) then
                call ray_box(origin, direction, bodies(i)%position, bodies(i)%orientation, &
                        bodies(i)%half_extents, best_t, cand_hit, cand_t, cand_point, cand_normal)
            else
                call ray_sphere(origin, direction, bodies(i)%position, bodies(i)%radius, &
                        best_t, cand_hit, cand_t, cand_point, cand_normal)
            end if
            if (cand_hit /= 0 .and. cand_t < best_t) then
                best_t = cand_t
                hit_found = 1
                hit_distance = cand_t
                hit_point = cand_point
                hit_normal = cand_normal
                hit_index = i - 1 ! 0-based — Rust-сторона переводит в handle
                hit_is_plane = 0
            end if
        end do

        do i = 1, n_planes
            call ray_plane(origin, direction, plane_normals(:, i), plane_points(:, i), &
                    best_t, cand_hit, cand_t, cand_point, cand_normal)
            if (cand_hit /= 0 .and. cand_t < best_t) then
                best_t = cand_t
                hit_found = 1
                hit_distance = cand_t
                hit_point = cand_point
                hit_normal = cand_normal
                hit_index = i - 1
                hit_is_plane = 1
            end if
        end do
    end subroutine raycast_query
end module raycast_mod
