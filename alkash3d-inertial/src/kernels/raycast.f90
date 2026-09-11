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
    ! ИЗМЕНЕНО (box-vs-box narrow phase — см. narrow_phase.f90): `rotate_
    ! vec_by_quat`/`quat_conjugate` переехали в `rigid_body_mod` (см. её
    ! комментарий) — были определены здесь же ДО того, как понадобились
    ! ещё и narrow_phase.f90, дублировать их там было бы копией того же
    ! кода.
    use rigid_body_mod, only: rigid_body_c, rotate_vec_by_quat, quat_conjugate
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
    ! ЛУЧ vs КАПСУЛА (см. shape_type::CAPSULE в lib.rs/rigid_body.f90)
    ! ===================================================================
    ! Точная (не приближение сэмплированием) аналитическая формула —
    ! стандартный приём Иниго Килеса для пересечения луча с капсулой
    ! (cм. его статьи про signed distance functions/raymarching, широко
    ! используемая и проверенная формула): сперва честно проверяем боковую
    ! (цилиндрическую) поверхность капсулы, и ТОЛЬКО если попадание пришлось
    ! мимо цилиндрической части (за пределами отрезка [pa,pb]) — проверяем
    ! соответствующую полусферу-крышку как обычную сферу.
    subroutine ray_capsule(origin, dir, pa, pb, radius, max_dist, hit, t, point, normal)
        real(c_float), intent(in) :: origin(3), dir(3), pa(3), pb(3), radius, max_dist
        integer(c_int), intent(out) :: hit
        real(c_float), intent(out) :: t, point(3), normal(3)

        real(c_float) :: ba(3), oa(3), baba, bard, baoa, rdoa, oaoa
        real(c_float) :: a, b, c, h, y
        integer(c_int) :: cap_hit
        real(c_float) :: cap_t, cap_point(3), cap_normal(3)

        hit = 0
        t = 0.0
        point = 0.0
        normal = 0.0

        ba = pb - pa
        oa = origin - pa
        baba = dot3(ba, ba)
        bard = dot3(ba, dir)
        baoa = dot3(ba, oa)
        rdoa = dot3(dir, oa)
        oaoa = dot3(oa, oa)

        a = baba - bard * bard
        if (abs(a) < RC_EPS) then
            ! Луч (почти) параллелен оси капсулы — цилиндрическая часть
            ! вырождена (нет единственного "бокового" пересечения),
            ! честно проверяем только обе полусферы-крышки ниже.
        else
            b = baba * rdoa - baoa * bard
            c = baba * oaoa - baoa * baoa - radius * radius * baba
            h = b * b - a * c
            if (h >= 0.0) then
                t = (-b - sqrt(h)) / a
                if (t >= 0.0 .and. t <= max_dist) then
                    y = baoa + t * bard
                    if (y > 0.0 .and. y < baba) then
                        ! Честное попадание в боковую (цилиндрическую)
                        ! поверхность — не в крышки.
                        hit = 1
                        point = origin + dir * t
                        normal = (oa + dir * t - ba * (y / baba)) / radius
                        return
                    end if
                end if
            end if
        end if

        ! Мимо цилиндрической части (или она вырождена) — проверяем обе
        ! полусферы-крышки как обычные сферы, берём ближайшее попадание.
        call ray_sphere(origin, dir, pa, radius, max_dist, cap_hit, cap_t, cap_point, cap_normal)
        if (cap_hit /= 0) then
            hit = 1
            t = cap_t
            point = cap_point
            normal = cap_normal
        end if
        call ray_sphere(origin, dir, pb, radius, max_dist, cap_hit, cap_t, cap_point, cap_normal)
        if (cap_hit /= 0) then
            if (hit == 0 .or. cap_t < t) then
                hit = 1
                t = cap_t
                point = cap_point
                normal = cap_normal
            end if
        end if
    end subroutine ray_capsule

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
        real(c_float) :: cap_axis_y(3), cap_p0(3), cap_p1(3)

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
            else if (bodies(i)%shape_type == 2) then
                ! Капсула (см. shape_type::CAPSULE) — half_extents(1) полу-
                ! высота вдоль локальной оси Y, см. `capsule_segment_world`
                ! в narrow_phase.f90 (та же формула, продублирована здесь
                ! как пара строк — не стоит ради неё межмодульной
                ! зависимости raycast_mod от narrow_phase_mod).
                cap_axis_y = rotate_vec_by_quat([0.0_c_float, 1.0_c_float, 0.0_c_float], bodies(i)%orientation)
                cap_p0 = bodies(i)%position + cap_axis_y * bodies(i)%half_extents(1)
                cap_p1 = bodies(i)%position - cap_axis_y * bodies(i)%half_extents(1)
                call ray_capsule(origin, direction, cap_p0, cap_p1, bodies(i)%radius, &
                        best_t, cand_hit, cand_t, cand_point, cand_normal)
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
