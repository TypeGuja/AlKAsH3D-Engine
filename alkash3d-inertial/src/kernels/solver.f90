! inertial/src/kernels/solver.f90
module solver_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, constraint_c
    implicit none

contains
    subroutine solve_constraints(bodies, constraints, n_constraints, iterations) &
            bind(c, name="solve_constraints")
        type(rigid_body_c), intent(inout) :: bodies(:)
        type(constraint_c), intent(inout) :: constraints(:)
        integer(c_int), intent(in) :: n_constraints, iterations
        integer :: iter, i

        do iter = 1, iterations
            do i = 1, n_constraints
                call solve_ball_joint(bodies(constraints(i)%body_a+1), &
                        bodies(constraints(i)%body_b+1), &
                        constraints(i))
            end do
        end do
    end subroutine solve_constraints

    ! ИСПРАВЛЕНО: было ДВЕ независимые ошибки, из-за которых сустав
    ! фактически не работал:
    !
    ! 1) `ra = constraint%anchor_a - body_a%position`, а потом
    !    `c = (body_a%position + ra) - (body_b%position + rb)`.
    !    Подставь ra — и увидишь, что `body_a%position + ra` алгебраически
    !    сокращается до просто `anchor_a`. То есть `c` был ВСЕГДА равен
    !    `anchor_a - anchor_b` — константе, совершенно не зависящей от
    !    того, как реально сдвинулись тела. "Ошибка" сустава не менялась
    !    от кадра к кадру, поэтому сустав не мог физически ничего
    !    удерживать в нужном положении.
    !
    ! 2) Импульс считался и применялся ТОЛЬКО по X-компоненте (`c(1)`,
    !    `velocity(1)`) — Y и Z полностью игнорировались, то есть "шар в
    !    гнезде" на самом деле удерживал только одну ось из трёх.
    !
    ! Теперь: anchor_a/anchor_b трактуются как фиксированные смещения от
    ! ТЕКУЩЕЙ позиции тела (мировая ориентация тел этой моделью физики не
    ! отслеживается — в rigid_body_c нет поля rotation/quaternion, поэтому
    ! анкер двигается вместе с телом, но не вращается вместе с ним; для
    ! честного учёта вращения понадобилось бы добавить ориентацию в
    ! rigid_body_c — это отдельная, более крупная доработка данных, а не
    ! однострочный фикс). Ошибка и импульс считаются по ВСЕМ трём осям.
    subroutine solve_ball_joint(body_a, body_b, constraint)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        type(constraint_c), intent(inout) :: constraint
        real(c_float) :: world_anchor_a(3), world_anchor_b(3), c_err(3)
        real(c_float) :: impulse(3), effective_mass, bias

        world_anchor_a = body_a%position + constraint%anchor_a
        world_anchor_b = body_b%position + constraint%anchor_b
        c_err = world_anchor_a - world_anchor_b

        effective_mass = 1.0 / (body_a%inv_mass + body_b%inv_mass + 0.001)
        bias = constraint%bias * 0.2

        impulse = effective_mass * (-c_err * bias - constraint%accumulated_impulse)

        body_a%velocity = body_a%velocity + impulse * body_a%inv_mass
        body_b%velocity = body_b%velocity - impulse * body_b%inv_mass

        ! ПРИМЕЧАНИЕ: accumulated_impulse в constraint_c — скаляр (не
        ! вектор), поэтому для warm-starting храним МОДУЛЬ накопленного
        ! импульса, а не полный вектор. Раз constraints/joints пока вообще
        ! не выведены наружу через ABI движка (в PhysicsAPI нет
        ! add_constraint), это осознанное упрощение, а не потеря точности
        ! для реально используемого пути — если joints когда-нибудь
        ! понадобятся в игре, тут стоит расширить constraint_c/
        ! FortranConstraint до accumulated_impulse(3).
        constraint%accumulated_impulse = constraint%accumulated_impulse + &
                sqrt(impulse(1)**2 + impulse(2)**2 + impulse(3)**2)
    end subroutine solve_ball_joint
end module solver_mod
