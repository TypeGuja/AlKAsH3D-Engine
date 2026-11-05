! inertial/src/kernels/softbody.f90
! Шаг деформируемого тела из узлов, балок и шарниров (разрушения при ударе).
! Описание модели — alkash3d-rust/src/softbody.rs, там же эталонная реализация на
! Rust и сверка с этой: порядок действий здесь совпадает с ней строка в строку,
! поэтому при правке меняются обе.
!
! Типы совпадают с #[repr(C)] на стороне Rust. Номера узлов и балок — с нуля.
module softbody_mod
    use, intrinsic :: iso_c_binding
    implicit none

    type, bind(c) :: sb_section
        real(c_double) :: ea
        real(c_double) :: yield_tension
        real(c_double) :: hardening
        real(c_double) :: peak_compression
        real(c_double) :: plateau_compression
        real(c_double) :: rupture_strain
        real(c_double) :: bottom_strain
        logical(c_bool) :: brittle
    end type

    type, bind(c) :: sb_beam
        integer(c_int) :: a, b
        real(c_double) :: rest0, rest, k, c
        type(sb_section) :: section
        logical(c_bool) :: buckled, broken
        real(c_double) :: plastic_strain
    end type

    type, bind(c) :: sb_hinge
        integer(c_int) :: a, m, c
        real(c_double) :: rest_angle, k, moment
        real(c_double) :: min_lever(2)
        integer(c_int) :: beams(2)
        logical(c_bool) :: broken
    end type

    ! kind: 0 — параллелепипед (bmin, bmax), 1 — вертикальный столб (x, z, radius, height)
    type, bind(c) :: sb_obstacle
        integer(c_int) :: kind
        real(c_double) :: bmin(3), bmax(3)
        real(c_double) :: x, z, radius, height
    end type

    integer, parameter :: PLASTIC = 1, DAMPING = 2, FRICTION = 3, RUPTURE = 4, CONTACT_DAMPING = 5

contains

    pure function cross3(a, b) result(r)
        real(c_double), intent(in) :: a(3), b(3)
        real(c_double) :: r(3)
        r(1) = a(2) * b(3) - a(3) * b(2)
        r(2) = a(3) * b(1) - a(1) * b(3)
        r(3) = a(1) * b(2) - a(2) * b(1)
    end function

    pure function dot3(a, b) result(r)
        real(c_double), intent(in) :: a(3), b(3)
        real(c_double) :: r
        r = a(1) * b(1) + a(2) * b(2) + a(3) * b(3)
    end function

    pure function len3(a) result(r)
        real(c_double), intent(in) :: a(3)
        real(c_double) :: r
        r = sqrt(dot3(a, a))
    end function

    subroutine softbody_step(n_nodes, pos, vel, force, mass, friction_k, radius, hits, &
            n_beams, beams, n_hinges, hinges, n_obstacles, obstacles, &
            n_pairs, pairs, self_radius, n_pole, pole, &
            has_ground, ground_y, gravity, dt, contact_k, contact_c, ledger) &
            bind(c, name="softbody_step")
        integer(c_int), value, intent(in) :: n_nodes, n_beams, n_hinges, n_obstacles, n_pairs, n_pole, has_ground
        real(c_double), intent(inout) :: pos(3, n_nodes), vel(3, n_nodes), force(3, n_nodes)
        real(c_double), intent(in) :: mass(n_nodes), friction_k(n_nodes), radius(n_nodes), self_radius(n_nodes)
        logical(c_bool), intent(in) :: hits(n_nodes)
        type(sb_beam), intent(inout) :: beams(n_beams)
        type(sb_hinge), intent(inout) :: hinges(n_hinges)
        type(sb_obstacle), intent(in) :: obstacles(n_obstacles)
        integer(c_int), intent(in) :: pairs(2, n_pairs), pole(2, n_pole)
        real(c_double), value, intent(in) :: ground_y, gravity, dt, contact_k, contact_c
        real(c_double), intent(inout) :: ledger(5)
        integer :: i

        force = 0.0_c_double
        call apply_beams(n_nodes, pos, vel, force, n_beams, beams, dt, ledger)
        call apply_hinges(n_nodes, pos, force, n_beams, beams, n_hinges, hinges, ledger)
        call apply_contacts(n_nodes, pos, vel, force, mass, friction_k, radius, self_radius, hits, &
                n_beams, beams, n_obstacles, obstacles, n_pairs, pairs, n_pole, pole, &
                has_ground, ground_y, dt, contact_k, contact_c, ledger)
        do i = 1, n_nodes
            vel(1, i) = vel(1, i) + (force(1, i) * (1.0_c_double / mass(i))) * dt
            vel(2, i) = vel(2, i) + (force(2, i) * (1.0_c_double / mass(i)) - gravity) * dt
            vel(3, i) = vel(3, i) + (force(3, i) * (1.0_c_double / mass(i))) * dt
            pos(1, i) = pos(1, i) + vel(1, i) * dt
            pos(2, i) = pos(2, i) + vel(2, i) * dt
            pos(3, i) = pos(3, i) + vel(3, i) * dt
        end do
    end subroutine

    subroutine apply_beams(n_nodes, pos, vel, force, n_beams, beams, dt, ledger)
        integer(c_int), intent(in) :: n_nodes, n_beams
        real(c_double), intent(in) :: pos(3, n_nodes), vel(3, n_nodes), dt
        real(c_double), intent(inout) :: force(3, n_nodes), ledger(5)
        type(sb_beam), intent(inout) :: beams(n_beams)
        integer :: i, ia, ib
        real(c_double) :: d(3), n(3), total(3), length, ext, f, rate, limit, flow, drop, bottom, strain, x, damp, dense

        do i = 1, n_beams
            if (beams(i)%broken) cycle
            ia = beams(i)%a + 1
            ib = beams(i)%b + 1
            d = pos(:, ib) - pos(:, ia)
            length = max(len3(d), 1.0e-6_c_double)
            n = d * (1.0_c_double / length)
            ext = length - beams(i)%rest
            f = beams(i)%k * ext
            rate = dot3(vel(:, ib) - vel(:, ia), n)
            if (f > 0.0_c_double) then
                limit = beams(i)%section%yield_tension + beams(i)%section%hardening * beams(i)%plastic_strain
                if (f > limit) then
                    flow = (f - limit) / beams(i)%k
                    ledger(PLASTIC) = ledger(PLASTIC) + limit * flow
                    beams(i)%rest = beams(i)%rest + flow
                    beams(i)%plastic_strain = beams(i)%plastic_strain + flow / beams(i)%rest0
                    f = limit
                end if
            else
                if (beams(i)%buckled) then
                    limit = beams(i)%section%plateau_compression
                else
                    limit = beams(i)%section%peak_compression
                end if
                if (-f > limit) then
                    flow = (-f - limit) / beams(i)%k
                    ledger(PLASTIC) = ledger(PLASTIC) + limit * flow
                    beams(i)%rest = beams(i)%rest - flow
                    if (.not. beams(i)%buckled) then
                        ! сброс упругой энергии при переходе с пика на плато уходит в течение
                        drop = max(limit - beams(i)%section%plateau_compression, 0.0_c_double) / beams(i)%k
                        ledger(PLASTIC) = ledger(PLASTIC) + (limit + beams(i)%section%plateau_compression) * 0.5_c_double * drop
                        beams(i)%rest = beams(i)%rest - drop
                        beams(i)%buckled = .true.
                    end if
                    f = -min(limit, beams(i)%section%plateau_compression)
                end if
            end if
            ! смята в плотный пакет: материалу некуда деваться, сила резко растёт
            bottom = beams(i)%rest0 * (1.0_c_double - beams(i)%section%bottom_strain)
            dense = beams(i)%k * 2.0_c_double
            if (length < bottom) f = f - dense * (bottom - length)
            ! разрыв: растянута больше предела, у стекла — и сжата
            strain = (length - beams(i)%rest0) / beams(i)%rest0
            if (strain > beams(i)%section%rupture_strain .or. &
                    (beams(i)%section%brittle .and. strain < -beams(i)%section%rupture_strain)) then
                x = length - beams(i)%rest
                ledger(RUPTURE) = ledger(RUPTURE) + 0.5_c_double * beams(i)%k * x * x
                if (length < bottom) ledger(RUPTURE) = ledger(RUPTURE) + 0.5_c_double * dense * (bottom - length)**2
                beams(i)%broken = .true.
                cycle
            end if
            damp = beams(i)%c * rate
            ledger(DAMPING) = ledger(DAMPING) + damp * rate * dt
            total = n * (f + damp)
            force(:, ia) = force(:, ia) + total
            force(:, ib) = force(:, ib) - total
        end do
    end subroutine

    subroutine apply_hinges(n_nodes, pos, force, n_beams, beams, n_hinges, hinges, ledger)
        integer(c_int), intent(in) :: n_nodes, n_beams, n_hinges
        real(c_double), intent(in) :: pos(3, n_nodes)
        real(c_double), intent(inout) :: force(3, n_nodes), ledger(5)
        type(sb_beam), intent(in) :: beams(n_beams)
        type(sb_hinge), intent(inout) :: hinges(n_hinges)
        integer :: i, ia, im, ic
        real(c_double) :: u(3), v(3), cr(3), n(3), fa(3), fc(3), lu, lv, cl, angle, torque, flow, s

        do i = 1, n_hinges
            if (hinges(i)%broken) cycle
            ia = hinges(i)%a + 1
            im = hinges(i)%m + 1
            ic = hinges(i)%c + 1
            u = pos(:, ia) - pos(:, im)
            v = pos(:, ic) - pos(:, im)
            if (beams(hinges(i)%beams(1) + 1)%broken .or. beams(hinges(i)%beams(2) + 1)%broken) then
                angle = atan2(len3(cross3(u, v)), dot3(u, v))
                ledger(RUPTURE) = ledger(RUPTURE) + 0.5_c_double * hinges(i)%k * (angle - hinges(i)%rest_angle)**2
                hinges(i)%broken = .true.
                cycle
            end if
            lu = len3(u)
            lv = len3(v)
            if (lu < 1.0e-5_c_double .or. lv < 1.0e-5_c_double) cycle
            cr = cross3(u, v)
            cl = len3(cr)
            ! почти прямая или полностью сложенная линия: направление изгиба не определено
            if (cl < 1.0e-9_c_double * lu * lv) cycle
            angle = atan2(cl, dot3(u, v))
            torque = hinges(i)%k * (angle - hinges(i)%rest_angle)
            if (abs(torque) > hinges(i)%moment) then
                flow = (abs(torque) - hinges(i)%moment) / hinges(i)%k
                ledger(PLASTIC) = ledger(PLASTIC) + hinges(i)%moment * flow
                s = sign(1.0_c_double, torque)
                hinges(i)%rest_angle = hinges(i)%rest_angle + flow * s
                torque = hinges(i)%moment * s
            end if
            n = cr * (1.0_c_double / cl)
            ! сила на концах плеч, перпендикулярно им, в сторону возврата угла к покою
            fa = cross3(n, u) * (torque / (lu * max(lu, hinges(i)%min_lever(1))))
            fc = cross3(v, n) * (torque / (lv * max(lv, hinges(i)%min_lever(2))))
            force(:, ia) = force(:, ia) + fa
            force(:, ic) = force(:, ic) + fc
            force(:, im) = force(:, im) - (fa + fc)
        end do
    end subroutine

    subroutine apply_contacts(n_nodes, pos, vel, force, mass, friction_k, radius, self_radius, hits, &
            n_beams, beams, n_obstacles, obstacles, n_pairs, pairs, n_pole, pole, &
            has_ground, ground_y, dt, k, c, ledger)
        integer(c_int), intent(in) :: n_nodes, n_beams, n_obstacles, n_pairs, n_pole, has_ground
        real(c_double), intent(in) :: pos(3, n_nodes), vel(3, n_nodes)
        real(c_double), intent(in) :: mass(n_nodes), friction_k(n_nodes), radius(n_nodes), self_radius(n_nodes)
        logical(c_bool), intent(in) :: hits(n_nodes)
        type(sb_beam), intent(in) :: beams(n_beams)
        type(sb_obstacle), intent(in) :: obstacles(n_obstacles)
        integer(c_int), intent(in) :: pairs(2, n_pairs), pole(2, n_pole)
        real(c_double), intent(in) :: ground_y, dt, k, c
        real(c_double), intent(inout) :: force(3, n_nodes), ledger(5)
        integer :: i, o, idx, ia, ib
        real(c_double) :: p(3), r, depth, normal(3), d, t, point(3), nv(3), vp(3), vn, fe, fd, f(3), dv(3), length
        real(c_double) :: dx, dz, l2, cx, cz
        logical :: touching

        do i = 1, n_nodes
            r = radius(i)
            p = pos(:, i)
            if (has_ground /= 0) then
                depth = ground_y + r - p(2)
                if (depth > 0.0_c_double) then
                    normal = [0.0_c_double, 1.0_c_double, 0.0_c_double]
                    call contact_static(i, normal, depth)
                end if
            end if
            if (.not. hits(i)) cycle
            do o = 1, n_obstacles
                if (obstacles(o)%kind == 0) then
                    call block_contact(p, r, obstacles(o)%bmin, obstacles(o)%bmax, normal, depth, touching)
                else
                    call pole_contact(p, r, obstacles(o), normal, depth, touching)
                end if
                if (touching) call contact_static(i, normal, depth)
            end do
        end do

        ! столб тоньше шага сетки — ещё и по балкам обшивки
        do idx = 1, n_pole
            associate (b => beams(pole(1, idx) + 1), ob => obstacles(pole(2, idx) + 1))
                if (b%broken) cycle
                ia = b%a + 1
                ib = b%b + 1
                dx = pos(1, ib) - pos(1, ia)
                dz = pos(3, ib) - pos(3, ia)
                l2 = dx * dx + dz * dz
                if (l2 > 1.0e-12_c_double) then
                    t = ((ob%x - pos(1, ia)) * dx + (ob%z - pos(3, ia)) * dz) / l2
                    t = max(0.0_c_double, min(1.0_c_double, t))
                else
                    t = 0.5_c_double
                end if
                cx = pos(1, ia) + dx * t
                cz = pos(3, ia) + dz * t
                d = sqrt((cx - ob%x)**2 + (cz - ob%z)**2)
                if (d >= ob%radius .or. t <= 0.02_c_double .or. t >= 0.98_c_double) cycle
                point = pos(:, ia) * (1.0_c_double - t) + pos(:, ib) * t
                if (point(2) > ob%height) cycle
                nv = [point(1) - ob%x, 0.0_c_double, point(3) - ob%z]
                normal = nv * (1.0_c_double / max(len3(nv), 1.0e-6_c_double))
                depth = ob%radius - d
                vp = vel(:, ia) * (1.0_c_double - t) + vel(:, ib) * t
                vn = dot3(vp, normal)
                fe = k * depth
                fd = 0.0_c_double
                if (vn < 0.0_c_double) fd = -c * vn
                ledger(CONTACT_DAMPING) = ledger(CONTACT_DAMPING) + fd * (-min(vn, 0.0_c_double)) * dt
                f = normal * (fe + fd)
                force(:, ia) = force(:, ia) + f * (1.0_c_double - t)
                force(:, ib) = force(:, ib) + f * t
            end associate
        end do

        ! узлы между собой
        do idx = 1, n_pairs
            ia = pairs(1, idx) + 1
            ib = pairs(2, idx) + 1
            dv = pos(:, ib) - pos(:, ia)
            length = max(len3(dv), 1.0e-6_c_double)
            depth = self_radius(ia) + self_radius(ib) - length
            if (depth <= 0.0_c_double) cycle
            normal = dv * (1.0_c_double / length)
            vn = dot3(vel(:, ib) - vel(:, ia), normal)
            fd = 0.0_c_double
            if (vn < 0.0_c_double) fd = -c * vn
            ledger(CONTACT_DAMPING) = ledger(CONTACT_DAMPING) + fd * (-min(vn, 0.0_c_double)) * dt
            f = normal * (k * depth + fd)
            force(:, ia) = force(:, ia) - f
            force(:, ib) = force(:, ib) + f
        end do

    contains

        subroutine contact_static(node, n, dep)
            integer, intent(in) :: node
            real(c_double), intent(in) :: n(3), dep
            real(c_double) :: v(3), vnn, fel, fdm, ftot, ff(3), vt(3), speed, ft
            v = vel(:, node)
            vnn = dot3(v, n)
            fel = k * dep
            fdm = 0.0_c_double
            if (vnn < 0.0_c_double) fdm = -c * vnn
            ledger(CONTACT_DAMPING) = ledger(CONTACT_DAMPING) + fdm * (-min(vnn, 0.0_c_double)) * dt
            ftot = fel + fdm
            ff = n * ftot
            ! трение Кулона со сглаживанием у нуля
            vt = v - n * vnn
            speed = len3(vt)
            if (speed > 1.0e-6_c_double) then
                ft = min(friction_k(node) * ftot, mass(node) * speed / dt * 0.5_c_double)
                ff = ff - vt * (ft / speed)
                ledger(FRICTION) = ledger(FRICTION) + ft * speed * dt
            end if
            force(:, node) = force(:, node) + ff
        end subroutine

    end subroutine

    pure subroutine block_contact(p, r, bmin, bmax, normal, depth, touching)
        real(c_double), intent(in) :: p(3), r, bmin(3), bmax(3)
        real(c_double), intent(out) :: normal(3), depth
        logical, intent(out) :: touching
        real(c_double) :: faces(6)
        integer :: i, best

        normal = 0.0_c_double
        depth = 0.0_c_double
        touching = p(1) > bmin(1) - r .and. p(1) < bmax(1) + r .and. p(2) > bmin(2) - r .and. p(2) < bmax(2) + r &
                .and. p(3) > bmin(3) - r .and. p(3) < bmax(3) + r
        if (.not. touching) return
        faces = [p(1) - (bmin(1) - r), (bmax(1) + r) - p(1), p(2) - (bmin(2) - r), (bmax(2) + r) - p(2), &
                p(3) - (bmin(3) - r), (bmax(3) + r) - p(3)]
        best = 1
        do i = 2, 6
            if (faces(i) < faces(best)) best = i
        end do
        depth = faces(best)
        i = (best + 1) / 2
        normal(i) = merge(-1.0_c_double, 1.0_c_double, mod(best, 2) == 1)
    end subroutine

    pure subroutine pole_contact(p, r, ob, normal, depth, touching)
        real(c_double), intent(in) :: p(3), r
        type(sb_obstacle), intent(in) :: ob
        real(c_double), intent(out) :: normal(3), depth
        logical, intent(out) :: touching
        real(c_double) :: d(3), length

        normal = 0.0_c_double
        depth = 0.0_c_double
        touching = .false.
        if (p(2) > ob%height) return
        d = [p(1) - ob%x, 0.0_c_double, p(3) - ob%z]
        length = len3(d)
        depth = ob%radius + r - length
        if (depth <= 0.0_c_double .or. length < 1.0e-6_c_double) return
        normal = d * (1.0_c_double / length)
        touching = .true.
    end subroutine

end module
