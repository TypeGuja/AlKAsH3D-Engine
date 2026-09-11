! inertial/src/kernels/narrow_phase.f90
module narrow_phase_mod
    use, intrinsic :: iso_c_binding
    ! ДОБАВЛЕНО (box-vs-box narrow phase): `rotate_vec_by_quat` нужен, чтобы
    ! развернуть локальные оси X/Y/Z коробки в мировые для SAT-теста — см.
    ! `narrow_phase_box_box` ниже. Определён в `rigid_body_mod` (см. её
    ! комментарий про общие кватернионные хелперы), не продублирован здесь.
    use rigid_body_mod, only: rigid_body_c, contact_c, rotate_vec_by_quat, quat_conjugate
    implicit none

    ! ИСПРАВЛЕНО: раньше здесь был "GJK с поддержкой любых форм", который
    ! на деле был жёстко зашит на сферы радиуса 0.5 (get_support_sphere),
    ! а при обнаружении столкновения penetration/normal просто ставились
    ! заглушками (penetration = 0.5 ВСЕГДА, normal — ненормализованный
    ! вектор между центрами) — EPA (для реальной глубины проникновения)
    ! не был реализован вообще, несмотря на объявленные константы
    ! EPA_MAX_ITER/EPA_MAX_FACES. Раз FortranRigidBody (и ABI PhysicsBody
    ! в движке) не несут никакой информации о форме тела — GJK/EPA общего
    ! назначения тут не нужен: ниже честный, корректный sphere-sphere
    ! тест с РЕАЛЬНОЙ глубиной проникновения и нормированной нормалью.
    !
    ! ИСПРАВЛЕНО (код-ревью): раньше радиус был одним захардкоженным
    ! `BODY_RADIUS` на ВСЕ тела без исключения. Теперь каждое тело несёт
    ! свой `%radius` (см. rigid_body_c) — сумма радиусов обоих тел вместо
    ! `BODY_RADIUS + BODY_RADIUS`.
    real(c_float), parameter :: MIN_DISTANCE = 1.0e-6

contains
    function narrow_phase_gjk(body_a, body_b, contact) result(collides) &
            bind(c, name="narrow_phase_gjk")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: body_a, body_b
        type(contact_c), intent(out) :: contact
        integer(c_int) :: collides

        real(c_float) :: delta(3), dist_sq, dist, radius_sum, inv_dist

        collides = 0
        contact%body_a = 0
        contact%body_b = 0
        contact%normal = [0.0, 0.0, 0.0]
        contact%penetration = 0.0
        contact%point = [0.0, 0.0, 0.0]

        ! ДОБАВЛЕНО (box-коллайдер кузова машины — см. `shape_type` в
        ! rigid_body_c): эта функция понимает ТОЛЬКО sphere-sphere.
        ! box-тела намеренно не должны сюда попадать (вызывающая
        ! Rust-сторона в lib.rs фильтрует их до вызова и считает
        ! box-vs-sphere/box-vs-plane сама), но защитный ранний выход на
        ! случай прямого вызова — честное "не пересекаются" вместо того,
        ! чтобы молча посчитать box как сферу радиуса `radius` (которое
        ! для box-тела не имеет физического смысла и не инициализируется).
        if (body_a%shape_type /= 0 .or. body_b%shape_type /= 0) then
            return
        end if

        delta = body_b%position - body_a%position
        dist_sq = delta(1)*delta(1) + delta(2)*delta(2) + delta(3)*delta(3)
        radius_sum = body_a%radius + body_b%radius

        if (dist_sq >= radius_sum * radius_sum) then
            return  ! не пересекаются
        end if

        dist = sqrt(max(dist_sq, MIN_DISTANCE))
        inv_dist = 1.0 / dist

        collides = 1
        ! Нормаль — от A к B, НОРМИРОВАННАЯ (раньше была не нормирована).
        contact%normal = delta * inv_dist
        ! Настоящая глубина проникновения (раньше — константа 0.5).
        contact%penetration = radius_sum - dist
        ! Точка контакта — на поверхности сферы A вдоль нормали к B.
        contact%point = body_a%position + contact%normal * body_a%radius
    end function narrow_phase_gjk

    ! ===================================================================
    ! ДОБАВЛЕНО (полноценная физика — box-vs-box narrow phase): раньше
    ! пара двух box-тел вообще не давала контакта (ни здесь, ни на
    ! Rust-стороне — `narrow_phase_box_sphere` в `lib.rs` понимает только
    ! box+sphere) — две коробки физически проходили бы друг сквозь друга.
    ! Классический 15-осевой SAT-тест (см. Ericson, "Real-Time Collision
    ! Detection", гл. 4.4): 3 оси граней A, 3 оси граней B, 9 осей
    ! попарных векторных произведений рёбер A×B. Если хотя бы одна из 15
    ! осей — разделяющая (проекции коробок на неё не пересекаются) —
    ! коробки не сталкиваются; иначе берём ось с МИНИМАЛЬНЫМ перекрытием
    ! (наименее "глубокая" ось — стандартный критерий выбора нормали
    ! контакта в SAT). Точка контакта — не честный манифолд (клиппинг
    ! граней), а ТОТ ЖЕ уровень упрощения, что уже используется по всему
    ! этому крейту (один контакт на пару тел, не несколько точек): середина
    ! отрезка между "опорными" вершинами обеих коробок вдоль нормали.
    ! ===================================================================
    pure function dot3_nb(a, b) result(d)
        real(c_float), intent(in) :: a(3), b(3)
        real(c_float) :: d
        d = a(1)*b(1) + a(2)*b(2) + a(3)*b(3)
    end function dot3_nb

    pure function cross3_nb(a, b) result(c)
        real(c_float), intent(in) :: a(3), b(3)
        real(c_float) :: c(3)
        c(1) = a(2)*b(3) - a(3)*b(2)
        c(2) = a(3)*b(1) - a(1)*b(3)
        c(3) = a(1)*b(2) - a(2)*b(1)
    end function cross3_nb

    ! Проецирует обе коробки на `axis` (ожидается НОРМИРОВАННОЙ) и
    ! возвращает перекрытие проекций (<= 0, если разделены — вызывающая
    ! сторона трактует это как честную разделяющую ось).
    pure function overlap_on_axis(axis, axes_a, he_a, axes_b, he_b, center_delta) result(overlap)
        real(c_float), intent(in) :: axis(3), axes_a(3,3), he_a(3), axes_b(3,3), he_b(3), center_delta(3)
        real(c_float) :: overlap
        real(c_float) :: ra, rb, dist
        ra = abs(dot3_nb(axis, axes_a(:,1))) * he_a(1) &
                + abs(dot3_nb(axis, axes_a(:,2))) * he_a(2) &
                + abs(dot3_nb(axis, axes_a(:,3))) * he_a(3)
        rb = abs(dot3_nb(axis, axes_b(:,1))) * he_b(1) &
                + abs(dot3_nb(axis, axes_b(:,2))) * he_b(2) &
                + abs(dot3_nb(axis, axes_b(:,3))) * he_b(3)
        dist = abs(dot3_nb(axis, center_delta))
        overlap = (ra + rb) - dist
    end function overlap_on_axis

    ! Вершина коробки, максимально "выступающая" в направлении `dir` —
    ! приближение точки контакта (не честный клиппинг граней, см. шапку
    ! `narrow_phase_box_box`).
    pure function box_support_point(center, axes, he, dir) result(pt)
        real(c_float), intent(in) :: center(3), axes(3,3), he(3), dir(3)
        real(c_float) :: pt(3)
        integer :: k
        pt = center
        do k = 1, 3
            if (dot3_nb(axes(:,k), dir) >= 0.0) then
                pt = pt + axes(:,k) * he(k)
            else
                pt = pt - axes(:,k) * he(k)
            end if
        end do
    end function box_support_point

    function narrow_phase_box_box(body_a, body_b, contact) result(collides) &
            bind(c, name="narrow_phase_box_box")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: body_a, body_b
        type(contact_c), intent(out) :: contact
        integer(c_int) :: collides

        real(c_float) :: axes_a(3,3), axes_b(3,3), center_delta(3)
        real(c_float) :: min_overlap, overlap, axis(3), best_normal(3), axis_len
        integer :: i, j

        collides = 0
        contact%body_a = 0
        contact%body_b = 0
        contact%normal = [0.0, 0.0, 0.0]
        contact%penetration = 0.0
        contact%point = [0.0, 0.0, 0.0]

        if (body_a%shape_type /= 1 .or. body_b%shape_type /= 1) return ! обе стороны обязаны быть коробками

        axes_a(:,1) = rotate_vec_by_quat([1.0, 0.0, 0.0], body_a%orientation)
        axes_a(:,2) = rotate_vec_by_quat([0.0, 1.0, 0.0], body_a%orientation)
        axes_a(:,3) = rotate_vec_by_quat([0.0, 0.0, 1.0], body_a%orientation)
        axes_b(:,1) = rotate_vec_by_quat([1.0, 0.0, 0.0], body_b%orientation)
        axes_b(:,2) = rotate_vec_by_quat([0.0, 1.0, 0.0], body_b%orientation)
        axes_b(:,3) = rotate_vec_by_quat([0.0, 0.0, 1.0], body_b%orientation)
        center_delta = body_b%position - body_a%position

        min_overlap = huge(1.0_c_float)
        best_normal = [0.0, 0.0, 0.0]

        ! 3 оси граней A + 3 оси граней B.
        do i = 1, 3
            overlap = overlap_on_axis(axes_a(:,i), axes_a, body_a%half_extents, axes_b, body_b%half_extents, center_delta)
            if (overlap <= 0.0) return ! честная разделяющая ось — коробки не пересекаются
            if (overlap < min_overlap) then
                min_overlap = overlap
                best_normal = axes_a(:,i)
            end if

            overlap = overlap_on_axis(axes_b(:,i), axes_a, body_a%half_extents, axes_b, body_b%half_extents, center_delta)
            if (overlap <= 0.0) return
            if (overlap < min_overlap) then
                min_overlap = overlap
                best_normal = axes_b(:,i)
            end if
        end do

        ! 9 осей попарных векторных произведений рёбер.
        do i = 1, 3
            do j = 1, 3
                axis = cross3_nb(axes_a(:,i), axes_b(:,j))
                axis_len = sqrt(dot3_nb(axis, axis))
                if (axis_len < 1.0e-6) cycle ! рёбра почти параллельны — вырожденная ось, SAT честно её пропускает
                axis = axis / axis_len

                overlap = overlap_on_axis(axis, axes_a, body_a%half_extents, axes_b, body_b%half_extents, center_delta)
                if (overlap <= 0.0) return
                if (overlap < min_overlap) then
                    min_overlap = overlap
                    best_normal = axis
                end if
            end do
        end do

        ! Нормаль — ОТ A К B (см. соглашение `narrow_phase_gjk` выше).
        if (dot3_nb(best_normal, center_delta) < 0.0) then
            best_normal = -best_normal
        end if

        collides = 1
        contact%normal = best_normal
        contact%penetration = min_overlap
        contact%point = 0.5 * ( &
                box_support_point(body_a%position, axes_a, body_a%half_extents, best_normal) + &
                box_support_point(body_b%position, axes_b, body_b%half_extents, -best_normal))
    end function narrow_phase_box_box

    ! ===================================================================
    ! ДОБАВЛЕНО (полноценная физика — box-vs-sphere в Fortran): перенос
    ! честного Rust-варианта (был `narrow_phase_box_sphere` в lib.rs) сюда
    ! — тот же алгоритм ("ближайшая точка на OBB к центру сферы": центр
    ! сферы переводится в локальные оси коробки, зажимается по
    ! half_extents, ближайшая точка переводится обратно в мировые
    ! координаты), только на Fortran-стороне, как и sphere-sphere/box-box
    ! выше — Rust-сторона (`lib.rs`) больше не считает узкую фазу сама, а
    ! только диспетчеризует, какую bind(c)-функцию вызвать по паре
    ! shape_type. `body_a`/`body_b` — В ТОМ ПОРЯДКЕ, в котором их передаёт
    ! вызывающий код (любой из них может оказаться коробкой) — normal в
    ! результате всегда "от body_a к body_b", тот же контракт, что у
    ! sphere-sphere/box-box.
    !
    ! ИЗВЕСТНОЕ УПРОЩЕНИЕ (унаследовано от Rust-версии): если центр сферы
    ! уже глубоко внутри коробки (сфера "телепортом" провалилась внутрь за
    ! один слишком быстрый кадр), выталкиваем по оси наименьшего
    ! проникновения относительно 6 граней — тот же приём, что и у
    ! `narrow_phase_box_box` выше, только для одной точки.
    function narrow_phase_box_sphere(body_a, body_b, contact) result(collides) &
            bind(c, name="narrow_phase_box_sphere")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: body_a, body_b
        type(contact_c), intent(out) :: contact
        integer(c_int) :: collides

        type(rigid_body_c) :: box_body, sphere_body
        logical :: box_is_a
        real(c_float) :: inv_q(4), rel(3), local_rel(3), clamped(3), delta_local(3)
        real(c_float) :: dist_sq, dist, inv_dist
        real(c_float) :: normal_local(3), penetration
        real(c_float) :: dx, dy, dz
        real(c_float) :: normal_world(3), closest_world_offset(3), point_world(3)

        collides = 0
        contact%body_a = 0
        contact%body_b = 0
        contact%normal = [0.0, 0.0, 0.0]
        contact%penetration = 0.0
        contact%point = [0.0, 0.0, 0.0]

        if (body_a%shape_type == 1 .and. body_b%shape_type /= 1) then
            box_body = body_a; sphere_body = body_b; box_is_a = .true.
        else if (body_b%shape_type == 1 .and. body_a%shape_type /= 1) then
            box_body = body_b; sphere_body = body_a; box_is_a = .false.
        else
            return ! обе стороны — коробки (см. narrow_phase_box_box) либо обе сферы (см. narrow_phase_gjk)
        end if

        inv_q = quat_conjugate(box_body%orientation)
        rel = sphere_body%position - box_body%position
        local_rel = rotate_vec_by_quat(rel, inv_q)

        clamped(1) = max(-box_body%half_extents(1), min(box_body%half_extents(1), local_rel(1)))
        clamped(2) = max(-box_body%half_extents(2), min(box_body%half_extents(2), local_rel(2)))
        clamped(3) = max(-box_body%half_extents(3), min(box_body%half_extents(3), local_rel(3)))
        delta_local = local_rel - clamped
        dist_sq = dot3_nb(delta_local, delta_local)

        if (dist_sq < 1.0e-8) then
            ! Центр сферы внутри коробки — см. шапку функции.
            dx = box_body%half_extents(1) - abs(local_rel(1))
            dy = box_body%half_extents(2) - abs(local_rel(2))
            dz = box_body%half_extents(3) - abs(local_rel(3))
            if (dx <= dy .and. dx <= dz) then
                normal_local = [sign(1.0_c_float, local_rel(1)), 0.0_c_float, 0.0_c_float]
                penetration = dx + sphere_body%radius
            else if (dy <= dz) then
                normal_local = [0.0_c_float, sign(1.0_c_float, local_rel(2)), 0.0_c_float]
                penetration = dy + sphere_body%radius
            else
                normal_local = [0.0_c_float, 0.0_c_float, sign(1.0_c_float, local_rel(3))]
                penetration = dz + sphere_body%radius
            end if
        else
            dist = sqrt(dist_sq)
            if (dist >= sphere_body%radius) return ! не пересекаются
            inv_dist = 1.0 / dist
            normal_local = delta_local * inv_dist
            penetration = sphere_body%radius - dist
        end if

        normal_world = rotate_vec_by_quat(normal_local, box_body%orientation)
        closest_world_offset = rotate_vec_by_quat(clamped, box_body%orientation)
        point_world = box_body%position + closest_world_offset

        collides = 1
        contact%point = point_world
        contact%penetration = penetration
        ! normal_world сейчас "от коробки к сфере" — контракт солвера
        ! требует "от body_a к body_b" (см. шапку функции).
        if (box_is_a) then
            contact%normal = normal_world
        else
            contact%normal = -normal_world
        end if
    end function narrow_phase_box_sphere

    ! ===================================================================
    ! ДОБАВЛЕНО (полноценная физика — box-vs-plane в Fortran): "насколько
    ! далеко коробка выступает в сторону `normal`" — support-функция OBB,
    ! нужная `resolve_plane_contacts` в lib.rs (та же формула, что раньше
    ! считалась там на Rust-стороне через `quat_rotate_vector`). Сумма
    ! |half_extent_i * (мировая ось_i · normal)| по трём локальным осям —
    ! точная (не приближение перебором 8 углов) величина того, что для
    ! сферы было бы просто `radius`, одинаковым по всем направлениям.
    function box_effective_radius(orientation, half_extents, normal) result(r) &
            bind(c, name="box_effective_radius")
        use, intrinsic :: iso_c_binding
        implicit none
        real(c_float), intent(in) :: orientation(4), half_extents(3), normal(3)
        real(c_float) :: r
        real(c_float) :: axis_x(3), axis_y(3), axis_z(3)

        axis_x = rotate_vec_by_quat([1.0_c_float, 0.0_c_float, 0.0_c_float], orientation)
        axis_y = rotate_vec_by_quat([0.0_c_float, 1.0_c_float, 0.0_c_float], orientation)
        axis_z = rotate_vec_by_quat([0.0_c_float, 0.0_c_float, 1.0_c_float], orientation)

        r = abs(half_extents(1) * dot3_nb(axis_x, normal)) &
                + abs(half_extents(2) * dot3_nb(axis_y, normal)) &
                + abs(half_extents(3) * dot3_nb(axis_z, normal))
    end function box_effective_radius
end module narrow_phase_mod
