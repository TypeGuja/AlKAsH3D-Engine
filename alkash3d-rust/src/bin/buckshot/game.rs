//! Правила Buckshot Roulette без графики — порт RoundManager /
//! DealerIntelligence / ItemManager / HealthCounter. Всё детерминировано
//! от сида (свой xorshift вместо `rand`, логику гоняют тесты без GPU).
//!
//! Данные раундов — ровно те, что лежат в main.tscn (round batch_0..2):
//! три "игры" по нарастающей, в каждой до пяти зарядок ружья.

pub const SLOTS: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Player,
    Dealer,
}

impl Side {
    pub fn other(self) -> Side {
        match self {
            Side::Player => Side::Dealer,
            Side::Dealer => Side::Player,
        }
    }
    pub fn idx(self) -> usize {
        match self {
            Side::Player => 0,
            Side::Dealer => 1,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Item {
    Handsaw,
    Magnifier,
    Beer,
    Cigarettes,
    Handcuffs,
}

impl Item {
    /// Порядок как в ItemManager.instanceArray (от него зависят префабы и смещения).
    pub const ALL: [Item; 5] = [Item::Handsaw, Item::Magnifier, Item::Beer, Item::Cigarettes, Item::Handcuffs];

    /// AmountResource.amount_main — сколько таких предметов может лежать у одной стороны.
    pub fn max_amount(self) -> usize {
        match self {
            Item::Cigarettes => 2,
            _ => 8,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Load {
    pub live: usize,
    pub blank: usize,
    /// shufflingArray: показывать патроны на столе вперемешку.
    pub shuffled_display: bool,
}

pub struct Batch {
    pub health: i32,
    pub items: usize,
    pub loads: [Load; 5],
}

const fn ld(live: usize, blank: usize, shuffled_display: bool) -> Load {
    Load { live, blank, shuffled_display }
}

pub const BATCHES: [Batch; 3] = [
    Batch { health: 2, items: 0, loads: [ld(1, 2, false), ld(3, 2, false), ld(3, 2, false), ld(3, 3, false), ld(5, 2, false)] },
    Batch { health: 4, items: 2, loads: [ld(1, 1, false), ld(2, 2, false), ld(3, 2, false), ld(3, 3, false), ld(5, 2, false)] },
    Batch { health: 6, items: 4, loads: [ld(1, 2, false), ld(4, 4, true), ld(3, 2, true), ld(4, 2, true), ld(5, 3, true)] },
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    UseItem(usize),
    Shoot(Side),
}

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng((seed ^ 0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    /// Равномерно в [lo, hi] включительно.
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + (self.next_u64() % (hi - lo + 1) as u64) as usize
    }
    pub fn coin(&mut self) -> bool {
        self.next_u64() & 1 == 1
    }
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() % 1_000_000) as f32 / 1_000_000.0
    }
    pub fn shuffle<T>(&mut self, v: &mut [T]) {
        for i in (1..v.len()).rev() {
            let j = self.range(0, i);
            v.swap(i, j);
        }
    }
}

/// Итог выстрела — всё, что нужно презентации.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shot {
    pub shooter: Side,
    pub target: Side,
    pub live: bool,
    pub damage: i32,
    /// Ход остаётся у стрелка (холостой в себя).
    pub again: bool,
}

pub struct Game {
    pub rng: Rng,
    pub batch: usize,
    pub load: usize,
    pub hp: [i32; 2],
    /// Очередь патронов: [0] — в патроннике. true = боевой.
    pub shells: Vec<bool>,
    pub items: [[Option<Item>; SLOTS]; 2],
    pub saw: bool,
    pub cuffed: [bool; 2],
    /// Скованный уже пропустил ход и в следующий раз освободится.
    pub about_to_break_free: [bool; 2],
    /// Что дилер знает о патроне в патроннике (лупа / последний патрон).
    pub dealer_knows: Option<bool>,
    /// Финал: провод дефибриллятора перерезан — лечения нет, смерть окончательная.
    pub wire_cut: [bool; 2],
    /// DealerIntelligence.dealerTarget: решение, в кого стрелять, принятое
    /// раньше выстрела (лупа, ножовка по монетке). Сбрасывается выстрелом.
    pub dealer_target: Option<Side>,
}

impl Game {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: Rng::new(seed),
            batch: 0,
            load: 0,
            hp: [BATCHES[0].health; 2],
            shells: Vec::new(),
            items: [[None; SLOTS]; 2],
            saw: false,
            cuffed: [false; 2],
            about_to_break_free: [false; 2],
            dealer_knows: None,
            wire_cut: [false; 2],
            dealer_target: None,
        }
    }

    pub fn max_hp(&self) -> i32 {
        BATCHES[self.batch].health
    }

    pub fn final_showdown(&self) -> bool {
        self.batch == 2
    }

    pub fn items_per_load(&self) -> usize {
        BATCHES[self.batch].items
    }

    pub fn live_left(&self) -> usize {
        self.shells.iter().filter(|s| **s).count()
    }

    pub fn blank_left(&self) -> usize {
        self.shells.len() - self.live_left()
    }

    /// MainBatchSetup: новая "игра" — здоровье, предметы со стола, наручники.
    pub fn start_batch(&mut self, batch: usize) {
        self.batch = batch;
        self.load = 0;
        self.hp = [self.max_hp(); 2];
        self.items = [[None; SLOTS]; 2];
        self.cuffed = [false; 2];
        self.about_to_break_free = [false; 2];
        self.wire_cut = [false; 2];
        self.saw = false;
        self.shells.clear();
    }

    /// StartRound + ShellSpawner: следующая зарядка. Возвращает порядок, в
    /// котором патроны лежат на столе (это не порядок в ружье).
    pub fn next_load(&mut self, first: bool) -> Vec<bool> {
        if !first && self.load + 1 < BATCHES[self.batch].loads.len() {
            self.load += 1;
        }
        // RemoveAllCuffsRoutine перед раздачей.
        self.cuffed = [false; 2];
        self.about_to_break_free = [false; 2];
        let l = BATCHES[self.batch].loads[self.load];
        let mut shown: Vec<bool> = (0..l.live).map(|_| true).chain((0..l.blank).map(|_| false)).collect();
        if l.shuffled_display {
            self.rng.shuffle(&mut shown);
        }
        let mut chamber = shown.clone();
        self.rng.shuffle(&mut chamber);
        self.rng.shuffle(&mut chamber);
        self.shells = chamber;
        self.dealer_knows = None;
        shown
    }

    pub fn free_slots(&self, side: Side) -> Vec<usize> {
        (0..SLOTS).filter(|&s| self.items[side.idx()][s].is_none()).collect()
    }

    fn count(&self, side: Side, item: Item) -> usize {
        self.items[side.idx()].iter().filter(|s| **s == Some(item)).count()
    }

    /// Случайный предмет из чемодана с учётом лимитов (ItemManager.GrabItem).
    pub fn draw_item(&mut self, side: Side) -> Option<Item> {
        let available: Vec<Item> = Item::ALL.iter().copied().filter(|&i| self.count(side, i) < i.max_amount()).collect();
        if available.is_empty() || self.free_slots(side).is_empty() {
            return None;
        }
        Some(available[self.rng.range(0, available.len() - 1)])
    }

    pub fn place_item(&mut self, side: Side, slot: usize, item: Item) {
        self.items[side.idx()][slot] = Some(item);
    }

    /// GrabItems_Enemy: дилер раскладывает свои предметы в случайные ячейки.
    pub fn deal_dealer_items(&mut self) -> Vec<(usize, Item)> {
        let mut placed = Vec::new();
        for _ in 0..self.items_per_load() {
            let free = self.free_slots(Side::Dealer);
            let Some(item) = self.draw_item(Side::Dealer) else { break };
            let slot = free[self.rng.range(0, free.len() - 1)];
            self.place_item(Side::Dealer, slot, item);
            placed.push((slot, item));
        }
        placed
    }

    pub fn can_use(&self, side: Side, slot: usize) -> bool {
        match self.items[side.idx()].get(slot).copied().flatten() {
            None => false,
            Some(Item::Handsaw) => !self.saw,
            Some(Item::Handcuffs) => !self.cuffed[side.other().idx()],
            Some(_) => !self.shells.is_empty(),
        }
    }

    /// Снимает предмет со стола и применяет мгновенную часть эффекта.
    /// Лечение сигаретами — отдельно (`heal`), оно происходит в момент из
    /// анимации, как в оригинале.
    pub fn use_item(&mut self, side: Side, slot: usize) -> Option<(Item, Option<bool>)> {
        if !self.can_use(side, slot) {
            return None;
        }
        let item = self.items[side.idx()][slot].take()?;
        let mut revealed = None;
        match item {
            Item::Magnifier => {
                revealed = Some(self.shells[0]);
                if side == Side::Dealer {
                    self.dealer_knows = Some(self.shells[0]);
                }
            }
            Item::Beer => {
                revealed = Some(self.shells.remove(0));
                self.dealer_knows = None;
            }
            Item::Handsaw => self.saw = true,
            Item::Handcuffs => {
                self.cuffed[side.other().idx()] = true;
                self.about_to_break_free[side.other().idx()] = false;
            }
            Item::Cigarettes => {}
        }
        Some((item, revealed))
    }

    /// UpdateDisplayRoutineCigarette: +1 заряд, если провод цел. true — здоровье выросло.
    pub fn heal(&mut self, side: Side) -> bool {
        let i = side.idx();
        if self.wire_cut[i] || self.hp[i] >= self.max_hp() {
            return false;
        }
        self.hp[i] += 1;
        true
    }

    pub fn shoot(&mut self, shooter: Side, target: Side) -> Shot {
        let live = self.shells.remove(0);
        let damage = if live { if self.saw { 2 } else { 1 } } else { 0 };
        self.saw = false;
        self.dealer_knows = None;
        self.dealer_target = None;
        let t = target.idx();
        self.hp[t] = (self.hp[t] - damage).max(0);
        Shot { shooter, target, live, damage, again: !live && target == shooter }
    }

    /// HealthCounter.UpdateDisplayRoutine в финале: при ≤2 зарядах провод
    /// режется, здоровье падает до 1. Возвращает стороны, у которых провод
    /// перерезан только что.
    pub fn check_wire(&mut self) -> Vec<Side> {
        let mut cut = Vec::new();
        if !self.final_showdown() {
            return cut;
        }
        for side in [Side::Dealer, Side::Player] {
            let i = side.idx();
            if self.hp[i] > 0 && self.hp[i] <= 2 && !self.wire_cut[i] {
                self.wire_cut[i] = true;
                self.hp[i] = 1;
                cut.push(side);
            }
        }
        cut
    }

    /// Порт DealerIntelligence.DealerChoice: следующее действие дилера.
    /// Перебирает свои предметы в порядке ячеек и берёт первый подходящий —
    /// как оригинал (порядок влияет на поведение).
    pub fn dealer_decide(&mut self) -> Action {
        let me = Side::Dealer.idx();
        if self.shells.len() == 1 {
            self.dealer_knows = Some(self.shells[0]);
        }
        let known = self.dealer_knows;
        for slot in 0..SLOTS {
            let Some(item) = self.items[me][slot] else { continue };
            let pick = match item {
                Item::Magnifier => known.is_none() && self.shells.len() != 1,
                Item::Cigarettes => self.hp[me] < self.max_hp(),
                Item::Beer => known != Some(true) && self.shells.len() != 1,
                Item::Handcuffs => !self.cuffed[Side::Player.idx()] && self.shells.len() != 1,
                Item::Handsaw => !self.saw && known == Some(true),
            };
            if pick && self.can_use(Side::Dealer, slot) {
                if item == Item::Handsaw {
                    self.dealer_target = Some(Side::Player);
                }
                return Action::UseItem(slot);
            }
        }
        if !self.saw && known != Some(false) {
            if let Some(slot) = (0..SLOTS).find(|&s| self.items[me][s] == Some(Item::Handsaw)) {
                // Монетка: либо пилит и потом стреляет в игрока, либо целится в себя.
                if self.rng.coin() {
                    self.dealer_target = Some(Side::Player);
                    return Action::UseItem(slot);
                }
                self.dealer_target = Some(Side::Dealer);
            }
        }
        if let Some(t) = self.dealer_target {
            if known.is_none() {
                return Action::Shoot(t);
            }
        }
        match known {
            Some(true) => Action::Shoot(Side::Player),
            Some(false) => Action::Shoot(Side::Dealer),
            None => {
                if self.rng.coin() { Action::Shoot(Side::Player) } else { Action::Shoot(Side::Dealer) }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loads_match_original_batches() {
        let mut g = Game::new(1);
        g.start_batch(1);
        let shown = g.next_load(true);
        assert_eq!(shown, vec![true, false]);
        assert_eq!((g.live_left(), g.blank_left()), (1, 1));
        for _ in 0..10 {
            g.next_load(false);
        }
        assert_eq!(g.load, 4, "после последней зарядки набор повторяется");
        assert_eq!((g.live_left(), g.blank_left()), (5, 2));
    }

    #[test]
    fn blank_on_self_keeps_turn() {
        let mut g = Game::new(1);
        g.start_batch(0);
        g.shells = vec![false, true];
        let s = g.shoot(Side::Player, Side::Player);
        assert!(s.again && !s.live);
        assert_eq!(g.hp, [2, 2]);
    }

    #[test]
    fn saw_doubles_damage_and_resets() {
        let mut g = Game::new(2);
        g.start_batch(2);
        g.items[0][0] = Some(Item::Handsaw);
        g.shells = vec![true, true, false];
        g.use_item(Side::Player, 0).unwrap();
        assert!(!g.can_use(Side::Player, 0));
        let s = g.shoot(Side::Player, Side::Dealer);
        assert_eq!(s.damage, 2);
        assert_eq!(g.hp[1], 4);
        assert!(!g.saw);
    }

    #[test]
    fn wire_cut_in_final_only() {
        let mut g = Game::new(3);
        g.start_batch(1);
        g.hp = [2, 2];
        assert!(g.check_wire().is_empty());
        g.start_batch(2);
        g.hp = [2, 5];
        assert_eq!(g.check_wire(), vec![Side::Player]);
        assert_eq!(g.hp[0], 1);
        assert!(!g.heal(Side::Player), "после обреза провода сигареты не лечат");
    }

    #[test]
    fn cigarette_limit() {
        let mut g = Game::new(4);
        g.start_batch(2);
        g.items[0][0] = Some(Item::Cigarettes);
        g.items[0][1] = Some(Item::Cigarettes);
        for _ in 0..200 {
            assert_ne!(g.draw_item(Side::Player), Some(Item::Cigarettes));
        }
    }

    #[test]
    fn dealer_uses_magnifier_then_acts_on_knowledge() {
        let mut g = Game::new(5);
        g.start_batch(1);
        g.items[1][3] = Some(Item::Magnifier);
        g.shells = vec![true, false, false];
        assert_eq!(g.dealer_decide(), Action::UseItem(3));
        g.use_item(Side::Dealer, 3);
        assert_eq!(g.dealer_decide(), Action::Shoot(Side::Player));
    }

    #[test]
    fn dealer_knows_last_shell() {
        let mut g = Game::new(6);
        g.start_batch(0);
        g.shells = vec![false];
        assert_eq!(g.dealer_decide(), Action::Shoot(Side::Dealer));
        g.shells = vec![true];
        g.dealer_knows = None;
        assert_eq!(g.dealer_decide(), Action::Shoot(Side::Player));
    }

    #[test]
    fn full_games_terminate() {
        for seed in 0..200 {
            let mut g = Game::new(seed);
            let mut batch = 0;
            g.start_batch(0);
            g.next_load(true);
            let mut turn = Side::Player;
            let mut steps = 0;
            loop {
                steps += 1;
                assert!(steps < 20_000, "seed {seed}: игра не заканчивается");
                if g.shells.is_empty() {
                    g.next_load(false);
                    for _ in 0..g.items_per_load() {
                        if let Some(item) = g.draw_item(Side::Player) {
                            let s = g.free_slots(Side::Player)[0];
                            g.place_item(Side::Player, s, item);
                        }
                    }
                    g.deal_dealer_items();
                    turn = Side::Player;
                }
                let action = if turn == Side::Dealer {
                    g.dealer_decide()
                } else if let Some(s) = (0..SLOTS).find(|&s| g.can_use(Side::Player, s)) {
                    Action::UseItem(s)
                } else {
                    Action::Shoot(Side::Dealer)
                };
                match action {
                    Action::UseItem(s) => {
                        let (item, _) = g.use_item(turn, s).expect("недопустимый предмет");
                        if item == Item::Cigarettes {
                            g.heal(turn);
                        }
                    }
                    Action::Shoot(target) => {
                        let shot = g.shoot(turn, target);
                        g.check_wire();
                        if g.hp[0] == 0 {
                            break;
                        }
                        if g.hp[1] == 0 {
                            batch += 1;
                            if batch == 3 {
                                break;
                            }
                            g.start_batch(batch);
                            g.next_load(true);
                            turn = Side::Player;
                            continue;
                        }
                        if !shot.again {
                            let next = turn.other();
                            if g.cuffed[next.idx()] {
                                g.cuffed[next.idx()] = false;
                            } else {
                                turn = next;
                            }
                        }
                    }
                }
            }
        }
    }
}
