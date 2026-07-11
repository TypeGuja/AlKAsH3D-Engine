// wrapper.h - заголовочный файл для генерации Rust биндингов
//
// ИСПРАВЛЕНО: раньше этот заголовок не совпадал с реальным layout'ом
// rigid_body_c из rigid_body.f90 (там есть inertia/inv_inertia 3x3,
// angular_acceleration, linear_damping/angular_damping — здесь их не
// было вообще). Если бы кто-то сгенерировал биндинги через bindgen из
// ЭТОГО заголовка, получившаяся структура не совпадала бы по размеру и
// смещениям полей с тем, что реально передаёт/читает Fortran-код — это
// был бы classic ABI mismatch (тихая порча памяти). Теперь структуры
// здесь побайтово совпадают с rigid_body_c и с FortranRigidBody в
// ffi/mod.rs.

typedef struct {
    float position[3];
    float velocity[3];
    float acceleration[3];
    float angular_velocity[3];
    float angular_acceleration[3];
    float inertia[3][3];
    float inv_inertia[3][3];
    float mass;
    float inv_mass;
    float restitution;
    float friction;
    float linear_damping;
    float angular_damping;
    int is_static;
    int is_asleep;
} FortranRigidBody;

typedef struct {
    int body_a;
    int body_b;
    float normal[3];
    float penetration;
    float point[3];
    float tangent1[3];
    float tangent2[3];
    float friction_impulse[2];
} FortranContact;

typedef struct {
    int body_a;
    int body_b;
    float anchor_a[3];
    float anchor_b[3];
    float bias;
    float accumulated_impulse;
} FortranConstraint;

// Broad phase
void broad_phase_grid(const FortranRigidBody* bodies, int n, float cell_size,
                       int grid_width, int grid_height,
                       int* cell_starts, int* cell_counts,
                       int* cell_pairs, int* pair_count);
// ИСПРАВЛЕНО: раньше здесь (и в ffi/mod.rs) отсутствовал параметр `n` —
// сигнатура не совпадала с реальным Fortran-кодом broad_phase_sap_optimized
// в broad_phase.f90 (bodies, n, active_indices, active_count, pairs, pair_count).
void broad_phase_sap_optimized(const FortranRigidBody* bodies, int n,
                                const int* active_indices, int active_count,
                                int* pairs, int* pair_count);
void broad_phase_temporal(const FortranRigidBody* bodies, int n,
                           const int* active_list, int active_count,
                           int* pairs, int* pair_count,
                           float radius, float time_step);
void update_aabb(const FortranRigidBody* bodies, int n, float* min_bounds, float* max_bounds, float radius);
void update_aabb_vectorized(const FortranRigidBody* bodies, int n, float* min_bounds, float* max_bounds, float radius);

// Narrow phase
int narrow_phase_gjk(const FortranRigidBody* body_a, const FortranRigidBody* body_b, FortranContact* contact);
void generate_collision_pairs(const FortranRigidBody* bodies, int n, int* pairs, int* pair_count, float radius);

// Solver
void integrate_bodies(FortranRigidBody* bodies, int n, float dt);
void batch_integrate(FortranRigidBody* bodies, int n, float dt, float gravity, int start_idx, int end_idx);
void solve_contacts(FortranRigidBody* bodies, FortranContact* contacts, int n_contacts, int iterations);
void solve_contacts_vectorized(FortranRigidBody* bodies, FortranContact* contacts, int n_contacts, int iterations, float dt);
void solve_constraints(FortranRigidBody* bodies, FortranConstraint* constraints, int n_constraints, int iterations);
void resolve_penetration_batch(FortranRigidBody* bodies, FortranContact* contacts, int n_contacts);

// ИСПРАВЛЕНО: добавлен параметр sleep_timers — без него функция не могла
// реализовать гистерезис (таймер "тело неподвижно уже N секунд") и была
// пустышкой, которая никогда никого не усыпляла.
void update_sleep_state(FortranRigidBody* bodies, int n, float dt, float sleep_threshold,
                         float sleep_time, float* sleep_timers);

void compute_center_of_mass(const FortranRigidBody* bodies, int n, float* center, float* total_mass);
