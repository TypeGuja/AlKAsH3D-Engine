! inertial/src/kernels/narrow_phase.f90
module narrow_phase_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, contact_c
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
end module narrow_phase_mod
