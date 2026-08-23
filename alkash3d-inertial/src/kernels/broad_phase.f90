! inertial/src/kernels/broad_phase.f90
module broad_phase_mod
    use, intrinsic :: iso_c_binding
    use rigid_body_mod, only: rigid_body_c
    implicit none

    type, bind(c) :: grid_cell_t
        integer(c_int) :: start_idx
        integer(c_int) :: count
    end type grid_cell_t

contains
    ! ===================================================================
    ! UNIFORM GRID BROAD PHASE - O(N) сложность
    ! ===================================================================
    subroutine broad_phase_grid(bodies, n, cell_size, grid_width, grid_height, &
            cell_starts, cell_counts, cell_pairs, pair_count, max_pairs) &
            bind(c, name="broad_phase_grid")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in), value :: n
        real(c_float), intent(in), value :: cell_size
        integer(c_int), intent(in), value :: grid_width, grid_height
        integer(c_int), intent(inout) :: cell_starts(grid_width, grid_height)
        integer(c_int), intent(inout) :: cell_counts(grid_width, grid_height)
        integer(c_int), intent(out) :: cell_pairs(*)
        integer(c_int), intent(out) :: pair_count
        ! ДОБАВЛЕНО: реальная ёмкость cell_pairs (в ПАРАХ, не в int'ах) —
        ! без этого параметра подпрограмма не могла знать, сколько
        ! реально можно записать, и писала без ограничения (см. ниже).
        !
        ! ИСПРАВЛЕНО (STATUS_ACCESS_VIOLATION при запуске): все скалярные
        ! intent(in)-параметры здесь ниже получили атрибут `value`. Без
        ! него bind(c)-функция Fortran по умолчанию принимает скаляры ПО
        ! ССЫЛКЕ (адрес), а Rust `extern "C"` передаёт их ПО ЗНАЧЕНИЮ
        ! (i32/f32 в регистре) — Fortran пытался разыменовать сами числа
        ! (100, 28...) как адреса и падал прямо на входе в функцию.
        integer(c_int), intent(in), value :: max_pairs

        integer :: i, j, k, x, y, cx, cy, start, count, idx
        integer :: neighbors(8)
        real(c_float) :: radius, min_x, max_x, min_z, max_z
        real(c_float) :: pos_x, pos_z
        ! ИСПРАВЛЕНО (главный баг, вызывавший STATUS_ACCESS_VIOLATION):
        ! раньше компактный список тел по ячейкам ("Проход 3") писался
        ! ПРЯМО В cell_pairs — в тот же массив, куда "Проход 4"
        ! ОДНОВРЕМЕННО писал результирующие пары, ЧИТАЯ при этом из тех
        ! же ячеек cell_pairs(i)/cell_pairs(j). Как только курсор записи
        ! (idx) догонял ещё не обработанные ячейки — он затирал те самые
        ! данные, которые ещё предстояло прочитать. Плюс idx ничем не
        ! ограничивался сверху, а реальное число пар (ячейка + все
        ! соседи, БЕЗ проверки расстояния) может заметно превышать тот
        ! запас, что выделяет Rust — то есть запись уходила ЗА ПРЕДЕЛЫ
        ! выделенного буфера. Теперь компактный список тел живёт в
        ! ОТДЕЛЬНОМ локальном массиве body_list — он никогда не
        ! пересекается с cell_pairs, и запись в cell_pairs дополнительно
        ! ограничена max_pairs.
        integer, allocatable :: body_list(:)
        ! ИСПРАВЛЕНО (STATUS_ACCESS_VIOLATION / "Index '0' ... below lower
        ! bound of 1" под -fcheck=all): курсор заполнения body_list по
        ! ячейкам — раньше использовалась схема "декремент cell_starts
        ! перед записью", а это требует, чтобы ПЕРЕД проходом 3
        ! cell_starts(x,y) указывал НА ОДНУ ПОЗИЦИЮ ПОСЛЕ последнего слота
        ! ячейки (start+count), а не на её начало. Здесь же
        ! cell_starts после префиксной суммы хранил именно НАЧАЛО (1-based
        ! start), поэтому самая первая запись в любую ячейку декрементировала
        ! start до start-1 — для первой ячейки грид (start=1) это индекс 0,
        ! вне границ Fortran-массива (1-based). Теперь используется ОТДЕЛЬНЫЙ
        ! курсор fill_cursor, инициализированный копией (корректных)
        ! cell_starts, и заполнение идёт ВПЕРЁД (запись, затем инкремент) —
        ! cell_starts при этом вообще не трогается и не требует "восстановления".
        integer, allocatable :: fill_cursor(:, :)

        radius = 0.6
        pair_count = 0
        allocate(body_list(max(n, 1)))
        allocate(fill_cursor(grid_width, grid_height))

        ! Сброс счётчиков ячеек
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = 0
                cell_counts(x, y) = 0
            end do
        end do

        ! Проход 1: Подсчёт объектов в каждой ячейке
        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle

            pos_x = bodies(i)%position(1)
            pos_z = bodies(i)%position(3)

            x = floor(pos_x / cell_size) + 1
            y = floor(pos_z / cell_size) + 1

            if (x >= 1 .and. x <= grid_width .and. &
                    y >= 1 .and. y <= grid_height) then
                cell_counts(x, y) = cell_counts(x, y) + 1
            end if
        end do

        ! Проход 2: Префиксные суммы для индексов
        idx = 0
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = idx
                idx = idx + cell_counts(x, y)
            end do
        end do

        ! Переводим префиксные суммы из 0-based в 1-based (индексация
        ! Fortran-массивов) — это ОКОНЧАТЕЛЬНЫЕ значения cell_starts,
        ! дальше они не меняются нигде, включая проход 3.
        do y = 1, grid_height
            do x = 1, grid_width
                cell_starts(x, y) = cell_starts(x, y) + 1
            end do
        end do

        ! Курсор заполнения стартует с тех же позиций, что и cell_starts,
        ! но именно fill_cursor двигается вперёд по мере вставки тел —
        ! cell_starts остаётся неизменным и корректным для прохода 4.
        fill_cursor = cell_starts

        ! Проход 3: Заполнение ячеек объектами — ТЕПЕРЬ В body_list, А НЕ
        ! В cell_pairs.
        do i = 1, n
            if (bodies(i)%is_asleep == 1) cycle

            pos_x = bodies(i)%position(1)
            pos_z = bodies(i)%position(3)

            x = floor(pos_x / cell_size) + 1
            y = floor(pos_z / cell_size) + 1

            if (x >= 1 .and. x <= grid_width .and. &
                    y >= 1 .and. y <= grid_height) then
                body_list(fill_cursor(x, y)) = i - 1
                fill_cursor(x, y) = fill_cursor(x, y) + 1
            end if
        end do

        ! Проход 4: Поиск коллизий (только соседние ячейки). Читаем ИЗ
        ! body_list, пишем В cell_pairs — теперь это два разных массива,
        ! и запись жёстко ограничена max_pairs, чтобы никогда не выйти за
        ! пределы буфера, выделенного вызывающей стороной.
        idx = 1
        pair_count = 0

        do y = 1, grid_height
            do x = 1, grid_width
                start = cell_starts(x, y)
                count = cell_counts(x, y)

                do i = start, start + count - 1
                    do j = i + 1, start + count - 1
                        pair_count = pair_count + 1
                        if (pair_count <= max_pairs) then
                            cell_pairs(idx) = body_list(i)
                            cell_pairs(idx + 1) = body_list(j)
                            idx = idx + 2
                        end if
                    end do
                end do

                neighbors = [x+1, y, x, y+1, x+1, y+1, x-1, y+1]
                do k = 1, 7, 2
                    cx = neighbors(k)
                    cy = neighbors(k+1)
                    if (cx >= 1 .and. cx <= grid_width .and. &
                            cy >= 1 .and. cy <= grid_height) then

                        start = cell_starts(cx, cy)
                        count = cell_counts(cx, cy)

                        do i = cell_starts(x, y), cell_starts(x, y) + cell_counts(x, y) - 1
                            do j = start, start + count - 1
                                pair_count = pair_count + 1
                                if (pair_count <= max_pairs) then
                                    cell_pairs(idx) = body_list(i)
                                    cell_pairs(idx + 1) = body_list(j)
                                    idx = idx + 2
                                end if
                            end do
                        end do
                    end if
                end do
            end do
        end do

        deallocate(body_list)
        deallocate(fill_cursor)
        ! ПРИМЕЧАНИЕ: если pair_count > max_pairs на выходе — значит,
        ! реальных пар оказалось больше, чем вместил буфер, и часть из
        ! них НЕ записана (но pair_count честно отражает истинное
        ! количество). Rust-обёртка (find_pairs_grid) обязана это
        ! проверить и, если нужно, перевызвать с бо́льшим буфером — см.
        ! комментарий в ffi/mod.rs.
    end subroutine broad_phase_grid

    ! ===================================================================
    ! ОПТИМИЗИРОВАННЫЙ SAP С АКТИВНЫМ СПИСКОМ
    ! ===================================================================
    subroutine broad_phase_sap_optimized(bodies, n, active_indices, active_count, &
            pairs, pair_count) &
            bind(c, name="broad_phase_sap_optimized")
        use, intrinsic :: iso_c_binding
        implicit none
        type(rigid_body_c), intent(in) :: bodies(n)
        integer(c_int), intent(in), value :: n
        integer(c_int), intent(in) :: active_indices(*)
        integer(c_int), intent(in), value :: active_count
        integer(c_int), intent(out) :: pairs(*)
        integer(c_int), intent(out) :: pair_count

        integer :: i, j, idx_i, idx_j
        real(c_float) :: min1(3), max1(3), min2(3), max2(3)
        real(c_float) :: radius
        radius = 0.6

        pair_count = 0

        do i = 1, active_count
            idx_i = active_indices(i)
            if (bodies(idx_i+1)%is_asleep == 1) cycle

            min1 = bodies(idx_i+1)%position - radius
            max1 = bodies(idx_i+1)%position + radius

            do j = i+1, active_count
                idx_j = active_indices(j)
                if (bodies(idx_j+1)%is_asleep == 1) cycle

                min2 = bodies(idx_j+1)%position - radius
                max2 = bodies(idx_j+1)%position + radius

                if (aabb_intersect(min1, max1, min2, max2)) then
                    pair_count = pair_count + 1
                    pairs(2*pair_count - 1) = idx_i
                    pairs(2*pair_count) = idx_j
                end if
            end do
        end do
    end subroutine broad_phase_sap_optimized

    function aabb_intersect(min1, max1, min2, max2) result(intersect)
        implicit none
        real(c_float), intent(in) :: min1(3), max1(3), min2(3), max2(3)
        logical :: intersect
        intersect = .false.
        if (max1(1) < min2(1) .or. max2(1) < min1(1)) return
        if (max1(2) < min2(2) .or. max2(2) < min1(2)) return
        if (max1(3) < min2(3) .or. max2(3) < min1(3)) return
        intersect = .true.
    end function aabb_intersect
end module broad_phase_mod
