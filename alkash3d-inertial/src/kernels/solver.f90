! inertial/src/kernels/solver.f90
module solver_mod
    use, intrinsic :: iso_c_binding
    implicit none

    type, bind(c) :: constraint_c
        integer(c_int) :: body_a
        integer(c_int) :: body_b
        real(c_float) :: anchor_a(3)
        real(c_float) :: anchor_b(3)
        real(c_float) :: bias
        real(c_float) :: accumulated_impulse
    end type constraint_c

contains
    ! Sequential Impulse Solver для ограничений (шарниры, пружины)
    subroutine solve_constraints(bodies, constraints, n_constraints, iterations) &
            bind(c, name="solve_constraints")
        use rigid_body_mod, only: rigid_body_c
        type(rigid_body_c), intent(inout) :: bodies(:)
        type(constraint_c), intent(inout) :: constraints(:)
        integer(c_int), intent(in) :: n_constraints, iterations

        integer :: iter, i
        real(c_float) :: impulse, jacobian(6)

        do iter = 1, iterations
            do i = 1, n_constraints
                call solve_ball_joint(bodies(constraints(i)%body_a+1), &
                        bodies(constraints(i)%body_b+1), &
                        constraints(i))
            end do
        end do
    end subroutine solve_constraints

    ! Шарнирное соединение (как подвеска машины)
    subroutine solve_ball_joint(body_a, body_b, constraint)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        type(constraint_c), intent(inout) :: constraint

        real(c_float) :: ra(3), rb(3), c(3), jacobian(6)
        real(c_float) :: impulse, effective_mass, bias

        ! Векторы от центров масс до точек соединения
        ra = constraint%anchor_a - body_a%position
        rb = constraint%anchor_b - body_b%position

        ! Ошибка соединения
        c = (body_a%position + ra) - (body_b%position + rb)

        ! Якобиан
        jacobian(1:3) = [1.0, 0.0, 0.0]
        jacobian(4:6) = cross_product(ra, [1.0, 0.0, 0.0])

        ! Эффективная масса
        effective_mass = 1.0 / (body_a%inv_mass + body_b%inv_mass + &
                dot_product(ra, matmul(body_a%inv_inertia, ra)) + &
                dot_product(rb, matmul(body_b%inv_inertia, rb)))

        ! Bias для исправления ошибки
        bias = constraint%bias * 0.2

        ! Импульс
        impulse = effective_mass * (-c(1) * bias - jacobian(1) * constraint%accumulated_impulse)

        ! Применяем импульс
        body_a%velocity = body_a%velocity + impulse * jacobian(1:3) * body_a%inv_mass
        body_b%velocity = body_b%velocity - impulse * jacobian(1:3) * body_b%inv_mass

        constraint%accumulated_impulse = constraint%accumulated_impulse + impulse
    end subroutine solve_ball_joint

    ! Линейная пружина (для подвески)
    subroutine solve_spring(body_a, body_b, rest_length, stiffness, damping)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        real(c_float), intent(in) :: rest_length, stiffness, damping

        real(c_float) :: delta(3), distance, force_magnitude
        real(c_float) :: rel_vel(3), vel_along

        delta = body_b%position - body_a%position
        distance = sqrt(delta(1)**2 + delta(2)**2 + delta(3)**2)

        if (distance == 0.0) return

        ! Сила Гука
        force_magnitude = stiffness * (distance - rest_length)

        ! Демпфирование
        rel_vel = body_b%velocity - body_a%velocity
        vel_along = (rel_vel(1)*delta(1) + rel_vel(2)*delta(2) + rel_vel(3)*delta(3)) / distance
        force_magnitude = force_magnitude + damping * vel_along

        ! Применяем силу
        delta = delta / distance
        body_a%acceleration = body_a%acceleration + delta * force_magnitude * body_a%inv_mass
        body_b%acceleration = body_b%acceleration - delta * force_magnitude * body_b%inv_mass
    end subroutine solve_spring

    function matmul(m, v) result(res)
        real(c_float), intent(in) :: m(3,3), v(3)
        real(c_float) :: res(3)

        res(1) = m(1,1)*v(1) + m(1,2)*v(2) + m(1,3)*v(3)
        res(2) = m(2,1)*v(1) + m(2,2)*v(2) + m(2,3)*v(3)
        res(3) = m(3,1)*v(1) + m(3,2)*v(2) + m(3,3)*v(3)
    end function matmul

    function cross_product(a, b) result(c)
        real(c_float), intent(in) :: a(3), b(3)
        real(c_float) :: c(3)

        c(1) = a(2)*b(3) - a(3)*b(2)
        c(2) = a(3)*b(1) - a(1)*b(3)
        c(3) = a(1)*b(2) - a(2)*b(1)
    end function cross_product
end module solver_mod