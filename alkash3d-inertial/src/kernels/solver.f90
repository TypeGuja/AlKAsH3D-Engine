! inertial/src/kernels/solver.f90
module solver_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c, constraint_c, &
            JOINT_BALL, JOINT_HINGE, JOINT_FIXED, JOINT_SLIDER
    implicit none

contains
    ! ДОБАВЛЕНО (разборка машины на детали — джойнты/constraint API):
    ! раньше это была ЕДИНСТВЕННАЯ реализация — прямой вызов
    ! solve_ball_joint без выбора типа соединения (constraint_c вообще не
    ! имел поля joint_type). Сейчас constraint_c — универсальная запись,
    ! способная описать шаровой шарнир/петлю/жёсткую сварку/ползун (см.
    ! JOINT_* в rigid_body.f90), и solve_constraints диспетчеризует по
    ! `joint_type`.
    subroutine solve_constraints(bodies, constraints, n_constraints, iterations) &
            bind(c, name="solve_constraints")
        ! См. подробный комментарий в rigid_body.f90/solve_contacts про
        ! assumed-shape (:) и про `value` на скалярах — те же два фикса
        ! здесь.
        type(rigid_body_c), intent(inout) :: bodies(*)
        type(constraint_c), intent(inout) :: constraints(n_constraints)
        integer(c_int), intent(in), value :: n_constraints, iterations
        integer :: iter, i

        ! ИСПРАВЛЕНО (найдено при добавлении break_impulse_*/удалении
        ! старого одиночного accumulated_impulse): раньше накопленный
        ! импульс НИКОГДА не обнулялся — рос неограниченно из кадра в
        ! кадр, потому что constraints — это persistent-массив на
        ! Rust-стороне (alkash3d-inertial/src/lib.rs), а не временный
        ! буфер одного кадра. Порог разрушения обязан сравниваться с
        ! ПИКОВОЙ нагрузкой ЭТОГО шага, а не с суммой всей истории
        ! соединения — иначе даже статически висящая, ничем не
        ! перегруженная деталь рано или поздно "отвалится" сама по себе
        ! просто от накопления мелких численных поправок за много кадров.
        ! Обнуляем оба вектора-накопителя в начале каждого вызова —
        ! `iterations` проходов ниже заново накапливают ровно то, что
        ! потребовалось, чтобы удержать соединение на ЭТОМ шаге.
        do i = 1, n_constraints
            constraints(i)%linear_impulse = 0.0
            constraints(i)%angular_impulse = 0.0
        end do

        do iter = 1, iterations
            do i = 1, n_constraints
                if (constraints(i)%is_broken /= 0) cycle

                select case (constraints(i)%joint_type)
                case (JOINT_HINGE)
                    call solve_point(bodies(constraints(i)%body_a + 1), &
                            bodies(constraints(i)%body_b + 1), constraints(i), .false.)
                    call solve_hinge_angular(bodies(constraints(i)%body_a + 1), &
                            bodies(constraints(i)%body_b + 1), constraints(i))
                case (JOINT_FIXED)
                    call solve_point(bodies(constraints(i)%body_a + 1), &
                            bodies(constraints(i)%body_b + 1), constraints(i), .false.)
                    call solve_fixed_angular(bodies(constraints(i)%body_a + 1), &
                            bodies(constraints(i)%body_b + 1), constraints(i))
                case (JOINT_SLIDER)
                    call solve_point(bodies(constraints(i)%body_a + 1), &
                            bodies(constraints(i)%body_b + 1), constraints(i), .true.)
                case default
                    ! JOINT_BALL и любое нераспознанное значение — старое
                    ! поведение (только точка крепления, вращение
                    ! свободно). Безопасный дефолт для structs, у которых
                    ! joint_type не был явно выставлен (нулевая
                    ! инициализация = JOINT_BALL).
                    call solve_point(bodies(constraints(i)%body_a + 1), &
                            bodies(constraints(i)%body_b + 1), constraints(i), .false.)
                end select
            end do
        end do

        ! Порог разрушения проверяется ПОСЛЕ всех итераций шага — то есть
        ! против итоговой суммарной impulse-коррекции всего шага, не
        ! промежуточного значения одной итерации.
        do i = 1, n_constraints
            if (constraints(i)%is_broken /= 0) cycle
            if (constraints(i)%break_impulse_linear > 0.0) then
                if (sqrt(sum(constraints(i)%linear_impulse**2)) > constraints(i)%break_impulse_linear) then
                    constraints(i)%is_broken = 1
                end if
            end if
            if (constraints(i)%is_broken == 0 .and. constraints(i)%break_impulse_angular > 0.0) then
                if (sqrt(sum(constraints(i)%angular_impulse**2)) > constraints(i)%break_impulse_angular) then
                    constraints(i)%is_broken = 1
                end if
            end if
        end do
    end subroutine solve_constraints

    ! ===================================================================
    ! ЛИНЕЙНАЯ ЧАСТЬ (точка крепления) — общая для BALL/HINGE/FIXED/SLIDER
    ! ===================================================================
    ! ИСПРАВЛЕНО (найдено `examples/joint_test.rs::test_fixed_holds_against_gravity`
    ! ПОСЛЕ первой версии этого рефакторинга): раньше (и в старом
    ! solve_ball_joint, откуда это было портировано) импульс считался
    ! ТОЛЬКО из позиционной ошибки — `impulse = effective_mass*(-c_err*bias)`,
    ! без какого-либо члена, гасящего ОТНОСИТЕЛЬНУЮ СКОРОСТЬ тел. Для
    ! статики, к которой прикреплено падающее тело, это означает: пока
    ! позиционная ошибка мала (тело только начало падать), коррекция
    ! мизерна — а гравитация КАЖДЫЙ кадр заново разгоняет тело вниз ДО
    ! того, как накопится сколь-нибудь заметная ошибка положения. Сустав
    ! в такой формуле — не жёсткая связь, а слабая пружина, и тело
    ! утекает сквозь него практически в свободном падении (проверено:
    ! y=5.0 -> y=-0.94 за 2 секунды — то есть падение почти без
    ! сопротивления). Стандартная (Baumgarte-стабилизированная)
    ! формула точечного констрейнта обязана включать ОБА члена — и
    ! позиционный bias, и требование "относительная скорость в точке
    ! крепления должна быть равна нулю" (что и держит соединение против
    ! ЛЮБОЙ непрерывной внешней силы, не только гравитации):
    !
    !   C = anchor_b - anchor_a  (должно -> 0)
    !   rel_vel = v_b - v_a      (должно -> -bias*C, т.е. 0 в равновесии)
    !   P = effective_mass * (-bias*C - rel_vel)
    !   v_a' = v_a - P*inv_mass_a
    !   v_b' = v_b + P*inv_mass_b
    !
    ! Вывод (проверка размерности импульса момента): при подстановке
    ! v_a'-v_b' в следующую итерацию относительная скорость сходится к
    ! -bias*C, то есть ошибка C экспоненциально затухает к нулю с
    ! коэффициентом, определяемым `bias`, — а не "утекает" под
    ! постоянно действующей внешней силой, потому что `rel_vel`
    ! пересчитывается заново на каждой из `iterations` итераций КАЖДОГО
    ! кадра и включает уже добавленную гравитацией скорость немедленно,
    ! в тот же кадр, а не с задержкой в один кадр, как было раньше.
    !
    ! ПРИМЕЧАНИЕ (известное упрощение, унаследованное от исходной
    ! реализации): импульс считается только через линейные скорости тел,
    ! БЕЗ учёта момента `r x impulse`, который смещение точки крепления
    ! от центра масс должно давать угловой скорости (честная реализация
    ! точечного констрейнта на смещённый якорь требует полной 6x6
    ! эффективной массы через Якобиан). Для игровых сценариев (болты,
    ! навесы, дверные петли на некрупных деталях) эта неточность
    ! незаметна, но если понадобится физически точная связь на большом
    ! плече — это следующий шаг доработки, не сделанный здесь сознательно
    ! ради ограниченного объёма задачи.
    subroutine solve_point(body_a, body_b, constraint, project_perp_to_axis)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        type(constraint_c), intent(inout) :: constraint
        logical, intent(in) :: project_perp_to_axis
        real(c_float) :: world_anchor_a(3), world_anchor_b(3), c_err(3), rel_vel(3)
        real(c_float) :: impulse(3), effective_mass, bias
        real(c_float) :: axis(3), axis_len

        world_anchor_a = body_a%position + constraint%anchor_a
        world_anchor_b = body_b%position + constraint%anchor_b
        c_err = world_anchor_b - world_anchor_a
        rel_vel = body_b%velocity - body_a%velocity

        if (project_perp_to_axis) then
            ! JOINT_SLIDER: вдоль оси ползуна смещение И скорость
            ! РАЗРЕШЕНЫ — убираем составляющую вдоль axis_a из ОБОИХ
            ! векторов перед решением, остаются только 2 перпендикулярные
            ! (запертые) степени свободы. Без проекции rel_vel сустав
            ! сопротивлялся бы и движению ВДОЛЬ оси тоже — противоречило
            ! бы самому смыслу ползуна.
            axis_len = sqrt(sum(constraint%axis_a**2))
            if (axis_len > 1.0e-6) then
                axis = constraint%axis_a / axis_len
                c_err = c_err - dot_product(c_err, axis) * axis
                rel_vel = rel_vel - dot_product(rel_vel, axis) * axis
            end if
        end if

        effective_mass = 1.0 / (body_a%inv_mass + body_b%inv_mass + 0.001)
        bias = constraint%bias * 0.2

        impulse = effective_mass * (-c_err * bias - rel_vel)

        body_a%velocity = body_a%velocity - impulse * body_a%inv_mass
        body_b%velocity = body_b%velocity + impulse * body_b%inv_mass

        constraint%linear_impulse = constraint%linear_impulse + impulse
    end subroutine solve_point

    ! ===================================================================
    ! УГЛОВАЯ ЧАСТЬ — JOINT_HINGE (заперты 2 из 3 вращательных DOF)
    ! ===================================================================
    ! Гасит составляющую относительной угловой скорости, ПЕРПЕНДИКУЛЯРНУЮ
    ! оси петли — вращение ВОКРУГ оси (например, открывание двери/капота)
    ! остаётся свободным, а увод оси петли "вбок" — нет.
    subroutine solve_hinge_angular(body_a, body_b, constraint)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        type(constraint_c), intent(inout) :: constraint
        real(c_float) :: axis(3), axis_len, w_rel(3), w_perp(3)
        real(c_float) :: inv_ia, inv_ib, eff_mass, impulse(3)

        axis_len = sqrt(sum(constraint%axis_a**2))
        if (axis_len < 1.0e-6) return
        axis = constraint%axis_a / axis_len

        w_rel = body_a%angular_velocity - body_b%angular_velocity
        w_perp = w_rel - dot_product(w_rel, axis) * axis

        ! Изотропный тензор инерции: все тела в этой физике строятся по
        ! сферическому приближению (см. IMPLICIT_RADIUS в
        ! alkash3d-inertial/src/lib.rs), поэтому ЛЮБОЙ диагональный
        ! элемент inv_inertia одинаков в любом направлении и годится как
        ! скаляр напрямую — тензор не нужно разворачивать в мировые оси.
        ! Если тела когда-нибудь получат неизотропную форму (не сферу), это
        ! место придётся пересчитать через честный повёрнутый тензор.
        inv_ia = body_a%inv_inertia(1, 1)
        inv_ib = body_b%inv_inertia(1, 1)
        eff_mass = 1.0 / (inv_ia + inv_ib + 0.001)

        impulse = -w_perp * eff_mass * constraint%bias

        body_a%angular_velocity = body_a%angular_velocity + impulse * inv_ia
        body_b%angular_velocity = body_b%angular_velocity - impulse * inv_ib

        constraint%angular_impulse = constraint%angular_impulse + impulse
    end subroutine solve_hinge_angular

    ! ===================================================================
    ! УГЛОВАЯ ЧАСТЬ — JOINT_FIXED (заперты все 3 вращательных DOF)
    ! ===================================================================
    ! Жёсткая сварка/болтовое соединение: обе детали обязаны вращаться с
    ! одинаковой угловой скоростью, пока соединение не разрушено — вместе
    ! с точечным ограничением из solve_point это делает пару тел
    ! неотличимой от одного жёсткого тела до момента break_impulse_*.
    subroutine solve_fixed_angular(body_a, body_b, constraint)
        type(rigid_body_c), intent(inout) :: body_a, body_b
        type(constraint_c), intent(inout) :: constraint
        real(c_float) :: w_rel(3), inv_ia, inv_ib, eff_mass, impulse(3)

        w_rel = body_a%angular_velocity - body_b%angular_velocity
        inv_ia = body_a%inv_inertia(1, 1)
        inv_ib = body_b%inv_inertia(1, 1)
        eff_mass = 1.0 / (inv_ia + inv_ib + 0.001)

        impulse = -w_rel * eff_mass * constraint%bias

        body_a%angular_velocity = body_a%angular_velocity + impulse * inv_ia
        body_b%angular_velocity = body_b%angular_velocity - impulse * inv_ib

        constraint%angular_impulse = constraint%angular_impulse + impulse
    end subroutine solve_fixed_angular
end module solver_mod
