//! Dungeon conqueror: work one dungeon's locked sections for the keys they
//! demand, clear the guardian, empty the treasure chest, then bank the next
//! run's keys until nightfall refills it.
//!
//! The descent itself is already built: `move {target, depth}` walks the
//! surface leg, the stair shafts and every door it holds a key for, and hands
//! back `Interrupted` when something worth fighting turns up on the way. What
//! is here is only the policy over it — which floor to be on, which key is
//! owed, and when the chest is worth walking to.
//!
//! The phase machine is smaller than it looks, because the key rules do the
//! sequencing on their own. `relevant_key_depth` answers "which lock stands
//! between me and deeper", and the whole run is: hold that key and descend,
//! or do not and sweep the four floors that drop it. Emptying the chest
//! spends the keys, which puts the very same rule back at the start of the
//! section — so banking the next cycle's keys needs no phase of its own.

use std::sync::Arc;
use std::time::{Duration, Instant};

use onlinerpg_shared::dungeon::{key_drop_floors, relevant_key_depth};
use onlinerpg_shared::Position;

use super::{bag_load_pct, fighter, junk_list, labels, loot_candidates, Step, WorkerConfig};
use crate::dungeon::{ChestKind, Dungeon};
use crate::state::SharedState;

/// How close to a sweep stop counts as having looked at it.
const STOP_ARRIVE_RANGE: f32 = 4.0;
/// How far from the chest its haul lands. The server scatters it over
/// `CHEST_LOOT_SCATTER_MAX` (3 m); the rest is the cell we opened it from.
const CHEST_LOOT_RADIUS: f32 = 6.0;
/// Ticks the chest's site gets, counting the wait for its drops to be
/// broadcast as well as the pickups. A chest pays a few items; one that will
/// not come up (too heavy, unreachable) must not hold the worker for the night.
pub(super) const CHEST_LOOT_TRIES: u32 = 16;
/// Consecutive ticks asking for the same leg before it counts as wedged.
/// Generous: a leg interrupted by a fight on the way is reissued unchanged,
/// and a busy floor can eat several in a row.
const LEG_TRIES: u32 = 12;

/// How long a halt stays quiet before the dungeoneer tries again. The player
/// may have levelled elsewhere, or the section may have repopulated.
const HALT_RETRY: Duration = Duration::from_secs(300);

/// Why the dungeoneer stopped, in the words of the thing the player changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Halt {
    /// The configured dungeon is not in the registry.
    NoDungeon,
    /// Monsters are down there, but none inside the level margin.
    OutOfDepth,
    /// Died `death_limit` times this cycle.
    DiedTooOften,
    /// The same descent has been asked for over and over and never lands.
    Stuck,
}

impl Halt {
    fn clause(self, cfg: &WorkerConfig) -> String {
        match self {
            Halt::NoDungeon => format!(
                "no dungeon is registered as \"{}\" — pick one in the panel",
                cfg.dungeon_id.as_deref().unwrap_or("")
            ),
            Halt::OutOfDepth => format!(
                "every monster down there is above your level + {} — raise the level margin or \
                 level up first",
                cfg.level_margin
            ),
            Halt::DiedTooOften => format!(
                "died {} times this cycle — waiting for the dungeons to reset",
                cfg.death_limit
            ),
            Halt::Stuck => {
                "it cannot get where it is going from where it stands — move the character \
                 clear and it will pick up again"
                    .to_string()
            }
        }
    }
}

/// What one nightly cycle carries between ticks.
#[derive(Debug, Default)]
pub(crate) struct Run {
    /// The nightfall this cycle belongs to; a change starts a fresh one.
    epoch: Option<i64>,
    deaths: u32,
    /// The chest is emptied and the haul not yet sold.
    resupply_due: bool,
    /// How the chest read last tick, so the moment it is emptied is caught
    /// once — a trip already taken must not be asked for again.
    chest_was_spent: bool,
    /// Where the emptied chest threw its haul, and how many pickups have been
    /// spent on it. The driver loop's own sweep only knows about kills.
    loot_site: Option<Position>,
    loot_tries: u32,
    /// Sweeping done, still underground: the way out comes before the shop.
    leaving: bool,
    /// Which way the section tour is walking. It turns around at the ends
    /// rather than jumping back across, so every hop is a single floor.
    sweep_up: bool,
    /// Which of this floor's sweep stops have been stood at, and which floor
    /// that tour belongs to. A set rather than a cursor: a fight drags us
    /// across the floor, and every stop the chase crossed is one that has
    /// been looked at.
    seen: Vec<bool>,
    sweep_depth: Option<u8>,
    /// Whether this floor's tour has seen a monster at all, and whether any
    /// of them were worth fighting. Both together are what tell "the floor is
    /// between respawns" from "this floor is above our weight".
    saw_any: bool,
    saw_eligible: bool,
    /// Consecutive floors swept end to end holding monsters but no eligible
    /// one. A whole section of those is what `OutOfDepth` means.
    barren_floors: u32,
    /// The last leg asked for and how many ticks running it has been the
    /// answer. Movement is blocking — a leg either lands or reports back — so
    /// the same one over and over is one that is not working. That is the
    /// stall neither the level margin nor the death count can see: the worker
    /// looks busy and its progress line never changes.
    leg: Option<String>,
    legs: u32,
    halted: Option<(Halt, Instant)>,
    /// The last progress line, so it is reported when it changes and not
    /// twice a second.
    note: String,
    note_fresh: bool,
}

impl Run {
    /// Nightfall: the dungeons reset and the chest owes its once-a-night
    /// again, so every verdict this cycle reached is void.
    pub(super) fn observe_epoch(&mut self, epoch: Option<i64>) {
        let Some(epoch) = epoch else { return };
        if self.epoch == Some(epoch) {
            return;
        }
        let fresh = self.epoch.is_some();
        self.epoch = Some(epoch);
        if fresh {
            self.deaths = 0;
            self.halted = None;
            self.barren_floors = 0;
            self.chest_was_spent = false;
            self.loot_site = None;
            self.leaving = false;
            self.sweep_up = false;
            self.restart_sweep();
        }
    }

    pub(super) fn died(&mut self) {
        self.deaths += 1;
    }

    /// Watch the chest flip to spent. That one moment starts the errand that
    /// follows it: sweep up what it threw on the floor, climb out, then sell.
    pub(super) fn observe_chest(&mut self, spent: bool, site: Option<Position>) {
        if spent && !self.chest_was_spent {
            self.resupply_due = true;
            self.loot_site = site;
            self.loot_tries = 0;
            self.leaving = true;
        }
        self.chest_was_spent = spent;
    }

    /// Whether the haul from an emptied chest is still unsold. Only once we
    /// are out: a merchant cannot be walked to from underground, and the
    /// errand would spend the trip failing to path out of the floor.
    pub(super) fn resupply_due(&self) -> bool {
        self.resupply_due && !self.leaving
    }

    /// A town trip is under way, so the errand that asked for it is answered
    /// whether or not the shop could help. Coming straight back would be an
    /// endless commute.
    pub(super) fn town_trip_started(&mut self) {
        self.resupply_due = false;
    }

    /// The progress line, when it says something new.
    pub(super) fn take_note(&mut self) -> Option<String> {
        self.note_fresh.then(|| {
            self.note_fresh = false;
            self.note.clone()
        })
    }

    fn say(&mut self, note: String) {
        if self.note != note {
            self.note = note;
            self.note_fresh = true;
        }
    }

    /// Count a leg, and say whether it has stopped being worth repeating.
    fn walking(&mut self, key: String) -> bool {
        if self.leg.as_deref() == Some(key.as_str()) {
            self.legs += 1;
        } else {
            self.leg = Some(key);
            self.legs = 1;
        }
        self.legs > LEG_TRIES
    }

    fn restart_sweep(&mut self) {
        self.seen.clear();
        self.sweep_depth = None;
        self.saw_any = false;
        self.saw_eligible = false;
    }

    /// Start this floor's tour, and tick off every stop we are standing at.
    /// Called before anything else decides the tick, so the ground a chase
    /// covers counts as swept — the tour used to check only the one stop it
    /// was walking to, which left every stop the fight crossed still owed and
    /// sent the sweep back over the path it had just walked.
    fn look_around(&mut self, stops: &[Position], depth: u8, me: Position) {
        if self.sweep_depth != Some(depth) || self.seen.len() != stops.len() {
            self.sweep_depth = Some(depth);
            self.seen = vec![false; stops.len()];
            self.saw_any = false;
            self.saw_eligible = false;
        }
        for (seen, stop) in self.seen.iter_mut().zip(stops) {
            *seen |= (me.x - stop.x).hypot(me.z - stop.z) <= STOP_ARRIVE_RANGE;
        }
    }

    /// The unswept stop to walk to next: the nearest one, never the next one
    /// in the layout's own order. Order alone had the tour crossing the floor
    /// to reach a cell it had been standing beside a moment earlier.
    fn next_stop(&self, stops: &[Position], me: Position) -> Option<Position> {
        stops
            .iter()
            .zip(&self.seen)
            .filter(|(_, seen)| !**seen)
            .map(|(stop, _)| stop)
            .min_by(|a, b| {
                (me.x - a.x)
                    .hypot(me.z - a.z)
                    .total_cmp(&(me.x - b.x).hypot(me.z - b.z))
            })
            .copied()
    }

    fn swept(&self) -> usize {
        self.seen.iter().filter(|seen| **seen).count()
    }

    fn halt(&mut self, why: Halt, cfg: &WorkerConfig) {
        if self.halted.map(|(h, _)| h) != Some(why) {
            self.halted = Some((why, Instant::now()));
            self.say(format!("Stopped: {}", why.clause(cfg)));
        }
    }

    /// Whether a halt is still in force. The window expiring clears it, so
    /// the next tick decides afresh.
    fn halted_now(&mut self) -> bool {
        match self.halted {
            Some((_, at)) if at.elapsed() < HALT_RETRY => true,
            Some(_) => {
                self.halted = None;
                self.barren_floors = 0;
                self.restart_sweep();
                false
            }
            None => false,
        }
    }
}

/// The dungeon this worker works: the configured one, else the shallowest
/// registered — the one a fresh character can actually finish.
fn target_dungeon(s: &SharedState, cfg: &WorkerConfig) -> Option<Arc<Dungeon>> {
    let asked = cfg
        .dungeon_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());
    if let Some(id) = asked {
        return s.dungeon_named(id);
    }
    let world = s.world_cache.read().ok()?;
    world
        .all_dungeons()
        .iter()
        .min_by_key(|d| d.max_depth())
        .map(Arc::clone)
}

/// How deep we stand in *this* dungeon. Zero on the surface, and zero inside
/// somebody else's dungeon — from there the way on is the same walk.
pub(crate) fn current_depth(s: &SharedState, dungeon: &Dungeon) -> u8 {
    if s.self_floor_level >= 0 {
        return 0;
    }
    match s.self_player.as_ref() {
        Some(p) if dungeon.footprint_contains(p.position.x, p.position.z) => {
            s.self_floor_level.unsigned_abs()
        }
        _ => 0,
    }
}

/// The lock standing between `depth` and deeper, its key item, and whether we
/// carry it. `None` when nothing below is locked.
pub(crate) fn key_owed(
    s: &SharedState,
    dungeon: &Dungeon,
    depth: u8,
) -> Option<(u8, String, bool)> {
    let need = relevant_key_depth(depth, dungeon.max_depth())?;
    let id = dungeon.key_item_id(need);
    let held = s.self_bag.iter().any(|i| i.item_def_id == id);
    Some((need, id, held))
}

/// Nothing left to do until nightfall: the chest is emptied and the deepest
/// lock's key is already back in the bag for the next one. Surplus keys buy
/// nothing — the chest spends one of each — so more farming is wasted work.
pub(crate) fn cycle_done(s: &SharedState, dungeon: &Dungeon) -> bool {
    if !s.treasure_chest_spent(&dungeon.id) {
        return false;
    }
    match key_owed(s, dungeon, dungeon.max_depth()) {
        Some((_, _, held)) => held,
        // Nothing is locked at all: the chest was the whole errand.
        None => true,
    }
}

/// Hand back a leg, unless this same one has stopped landing.
fn leg(step: Step, cfg: &WorkerConfig, run: &mut Run) -> Vec<Step> {
    if run.walking(format!("{step:?}")) {
        if run.halted.is_none() {
            run.halt(Halt::Stuck, cfg);
        }
        return vec![Step::Idle];
    }
    vec![step]
}

fn descend(
    s: &SharedState,
    dungeon: &Dungeon,
    depth: u8,
    cfg: &WorkerConfig,
    run: &mut Run,
) -> Vec<Step> {
    let mut steps = leg(
        Step::Descend {
            dungeon: dungeon.name.clone(),
            depth,
        },
        cfg,
        run,
    );
    if matches!(steps.first(), Some(Step::Descend { .. })) {
        if let Some(reins) = reins_for_approach(s, dungeon) {
            steps.insert(0, Step::Use(reins));
        }
    }
    steps
}

/// The reins, when the surface leg to the entrance is long enough to pay for
/// climbing on. That leg is the only part of the trip a horse can carry: the
/// server dismounts us as soon as we are below ground, which is exactly where
/// the stair leg starts. Standing on the dungeon's own footprint means the
/// executor skips the surface leg altogether, so there is nothing to ride.
/// Everything else the mount needs — ground level, out of combat, out of a
/// house — `reins_for_leg` asks on its own.
fn reins_for_approach(s: &SharedState, dungeon: &Dungeon) -> Option<String> {
    let me = s.self_player.as_ref()?;
    if dungeon.footprint_contains(me.position.x, me.position.z) {
        return None;
    }
    super::reins_for_leg(s, dungeon.entrance.x, dungeon.entrance.z)
}

/// Out of the dungeon and waiting: the surface is where a stopped or finished
/// dungeoneer stands, not a corridor with monsters walking through it.
fn surface(
    s: &SharedState,
    dungeon: &Dungeon,
    depth: u8,
    cfg: &WorkerConfig,
    run: &mut Run,
) -> Vec<Step> {
    if depth == 0 {
        return vec![Step::Idle];
    }
    descend(s, dungeon, 0, cfg, run)
}

/// One tick's decision.
pub(crate) fn step(
    s: &mut SharedState,
    cfg: &WorkerConfig,
    run: &mut Run,
    labels: &labels::BagLabels,
) -> Vec<Step> {
    // Transit by default. A leg walked to get somewhere — down a floor, to
    // the chest, back out — is not worth abandoning for whatever wandered
    // into sight, and the descent stuttered between the stairs and a monster
    // it kept losing when it was. `farm` arms this where kills *are* the
    // errand, and arms `free_kill` with it: the two must always agree, or an
    // abandoned leg is re-decided unchanged and abandoned again.
    //
    // Being hit is still a fight — the driver loop's retaliation is ahead of
    // this and answers whatever actually strikes us.
    s.abandon_leg_for = None;
    let Some(dungeon) = target_dungeon(s, cfg) else {
        run.halt(Halt::NoDungeon, cfg);
        return vec![Step::Idle];
    };
    let depth = current_depth(s, &dungeon);
    run.observe_chest(
        s.treasure_chest_spent(&dungeon.id),
        dungeon.treasure_position(),
    );

    if run.deaths >= cfg.death_limit {
        run.halt(Halt::DiedTooOften, cfg);
    }
    if run.halted_now() {
        return surface(s, &dungeon, depth, cfg, run);
    }

    // Death puts the character in a bed on a building's first storey, and
    // from up there nothing works: a plain walk is refused as "not on the
    // storey you are standing on", and a descent skips its surface leg (the
    // bed is inside the dungeon's own footprint) to path from an upstairs
    // room into solid rock. The stairs down come first, and the dungeon's
    // doorstep is where we were going anyway.
    if s.self_floor_level > 0 {
        run.say(format!(
            "Indoors upstairs — taking the stairs down and heading for {}.",
            dungeon.name
        ));
        // The spawn point rather than the dungeon's doorstep: it is the one
        // outdoor cell the world guarantees is standable, and the descent's
        // own surface leg covers the rest of the walk from there.
        let (x, z) = fighter::spawn_point();
        return leg(Step::ToGround { x, z }, cfg, run);
    }

    // The bag is the only thing down here that can refuse a pickup — the
    // server leaves what will not fit on the floor — and a key or the chest's
    // haul refused is the errand lost. There is no merchant underground, so
    // the marked junk goes on the floor where we stand.
    if depth > 0 && bag_load_pct(s) >= cfg.bag_full_pct {
        if let Some(id) = junk_list(s, labels).into_iter().next() {
            run.say(format!("Bag full underground — dropping {id}."));
            return vec![Step::Drop(id)];
        }
    }

    // The chest bursts its haul onto the floor around itself. The driver
    // loop's sweep only follows kills, so the chest's site gets its own.
    if let Some(site) = run.loot_site {
        if run.loot_tries < CHEST_LOOT_TRIES {
            run.loot_tries += 1;
            if let Some(&id) = loot_candidates(s, site, CHEST_LOOT_RADIUS).first() {
                run.say(format!(
                    "{} — gathering what the chest threw ({}/{CHEST_LOOT_TRIES}).",
                    dungeon.name, run.loot_tries
                ));
                return vec![Step::Pickup(id)];
            }
            // The drops are broadcast a moment after the open is answered, so
            // an empty first look is the message still in flight, not a bare
            // floor. Giving up on it emptied every chest into thin air.
            run.say(format!("{} — waiting on the chest's haul.", dungeon.name));
            return vec![Step::Idle];
        }
        run.loot_site = None;
    }

    // Haul gathered: out of the dungeon before anything else. The town trip
    // waiting behind this cannot be walked from a floor underground.
    if run.leaving {
        if depth > 0 {
            run.say(format!("{} — haul gathered, heading out.", dungeon.name));
            return surface(s, &dungeon, depth, cfg, run);
        }
        run.leaving = false;
        return vec![Step::Idle];
    }

    if cycle_done(s, &dungeon) {
        run.say(format!(
            "{} done for tonight — chest emptied, {} banked. Waiting for the reset.",
            dungeon.name,
            key_owed(s, &dungeon, dungeon.max_depth())
                .map_or_else(|| "no key needed".to_string(), |(_, id, _)| id)
        ));
        return surface(s, &dungeon, depth, cfg, run);
    }

    // The boss floor, with the chest still owing: walk in and open it. The
    // guardian charges on sight and the retaliation rule in the driver loop
    // answers it whatever the level margin says, so the fight happens on its
    // own — this only has to keep asking for the chest.
    //
    // Once it is spent the floor holds nothing: the keys went with the open,
    // so the rule below reads the deepest lock as owed again and turns us
    // round into the section that drops it.
    if depth == dungeon.max_depth() && !s.treasure_chest_spent(&dungeon.id) {
        return claim(s, &dungeon, run);
    }

    match key_owed(s, &dungeon, depth) {
        // Nothing locked below, or the key is in the bag: keep going down.
        None | Some((_, _, true)) => {
            let next = (depth + 1).min(dungeon.max_depth());
            run.restart_sweep();
            run.say(format!("{} — descending to floor {next}.", dungeon.name));
            descend(s, &dungeon, next, cfg, run)
        }
        Some((lock, key, false)) => farm(s, cfg, &dungeon, depth, lock, &key, run),
    }
}

/// On the deepest floor with the chest unclaimed: walk to it and open it.
fn claim(s: &SharedState, dungeon: &Dungeon, run: &mut Run) -> Vec<Step> {
    // The guardian, if it still stands. Sight underground is room-bound, so
    // seeing it means we are in the boss room with it.
    let boss_up = s
        .nearby_monsters
        .values()
        .any(|m| m.monster_type == dungeon.boss_type() && m.health > 0);

    if s.chests_in_sight()
        .iter()
        .any(|c| c.kind == ChestKind::Treasure)
    {
        run.say(format!(
            "{} floor {} — {}opening the great chest.",
            dungeon.name,
            dungeon.max_depth(),
            if boss_up { "guardian up, " } else { "" }
        ));
        return vec![Step::OpenChest];
    }

    // The cell beside the chest, never the chest's own: it is a collision
    // pillar, and a walk aimed at it can only ever stop short — which reads
    // from in here as a chamber we never reached and a chest never opened.
    match dungeon.treasure_approach() {
        Some(spot) => {
            run.say(format!(
                "{} floor {} — walking to the guardian's chamber.",
                dungeon.name,
                dungeon.max_depth()
            ));
            vec![Step::Walk {
                x: spot.x,
                z: spot.z,
            }]
        }
        None => vec![Step::Idle],
    }
}

/// Missing `lock`'s key: work the four floors that drop it.
fn farm(
    s: &mut SharedState,
    cfg: &WorkerConfig,
    dungeon: &Dungeon,
    depth: u8,
    lock: u8,
    key: &str,
    run: &mut Run,
) -> Vec<Step> {
    let section = key_drop_floors(lock);
    let (first, last) = (*section.start(), *section.end());
    let span = (last - first + 1) as u32;
    // Outside the section — on the surface, or shallower/deeper than it
    // reaches. One floor toward it, never a jump: see `next_floor`.
    if !section.contains(&depth) {
        let goal = if depth < first { depth + 1 } else { depth - 1 };
        run.restart_sweep();
        run.say(format!(
            "{} — need {key}, working floors {first}-{last}.",
            dungeon.name
        ));
        return descend(s, dungeon, goal, cfg, run);
    }

    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return vec![Step::Idle];
    };
    let stops = dungeon.sweep_stops(depth);
    run.look_around(&stops, depth, me);

    // On the ground the key drops from: kills are the errand here, so a leg
    // is worth dropping the moment something worth hitting is in reach.
    s.abandon_leg_for = Some(cfg.level_margin);
    if let Some(id) = fighter::free_kill(s, cfg) {
        run.saw_eligible = true;
        run.saw_any = true;
        return vec![Step::Attack(id)];
    }

    run.saw_any |= s
        .nearby_monsters
        .values()
        .any(|m| m.floor_level == s.self_floor_level && m.health > 0);

    // Something worth walking to. Everything within striking range was
    // answered before this, so close most of the way and let the next tick's
    // chase finish it.
    if let Some(target) =
        fighter::target_unleashed(s, cfg).and_then(|id| s.nearby_monsters.get(&id))
    {
        run.saw_eligible = true;
        run.say(format!(
            "{} floor {depth} — hunting for {key}.",
            dungeon.name
        ));
        return vec![approach(s, target.position)];
    }

    // Nothing in sight: sight underground reaches one room, so walk the tour
    // of the cells this floor spawns monsters in.
    if let Some(stop) = run.next_stop(&stops, me) {
        run.say(format!(
            "{} floor {depth} — sweeping for {key} ({}/{}).",
            dungeon.name,
            run.swept() + 1,
            stops.len()
        ));
        return leg(
            Step::Walk {
                x: stop.x,
                z: stop.z,
            },
            cfg,
            run,
        );
    }

    // Tour finished. A floor that held monsters but offered none worth
    // fighting is evidence we are out of our depth; a floor that held nothing
    // at all is just between respawns, and says nothing either way.
    if run.saw_any && !run.saw_eligible {
        run.barren_floors += 1;
    } else if run.saw_eligible {
        run.barren_floors = 0;
    }
    if run.barren_floors >= span {
        run.halt(Halt::OutOfDepth, cfg);
        return vec![Step::Idle];
    }

    run.restart_sweep();
    let goal = next_floor(depth, first, last, run);
    descend(s, dungeon, goal, cfg, run)
}

/// The next floor of the section to sweep: one floor on, turning around at
/// either end. Never a jump back across the section — that leg crosses whole
/// floors of monsters, and hunting is exactly the phase a leg is abandoned
/// for prey, so it was dropped partway every time. The tour then restarted
/// wherever it stopped and the sweep shuttled over the last two floors
/// instead of covering all four.
pub(super) fn next_floor(depth: u8, first: u8, last: u8, run: &mut Run) -> u8 {
    if first >= last {
        return depth;
    }
    if run.sweep_up {
        if depth <= first {
            run.sweep_up = false;
            return depth + 1;
        }
        return depth - 1;
    }
    if depth >= last {
        run.sweep_up = true;
        return depth - 1;
    }
    depth + 1
}

/// Where to stop when closing on a target: inside striking range, not on top
/// of it.
const CLOSE_TO: f32 = 8.0;

fn approach(s: &SharedState, target: Position) -> Step {
    let Some(me) = s.self_player.as_ref().map(|p| p.position) else {
        return Step::Idle;
    };
    let (dx, dz) = (target.x - me.x, target.z - me.z);
    let dist = dx.hypot(dz);
    if dist <= CLOSE_TO {
        return Step::Walk {
            x: target.x,
            z: target.z,
        };
    }
    let ratio = (dist - CLOSE_TO) / dist;
    Step::Walk {
        x: me.x + dx * ratio,
        z: me.z + dz * ratio,
    }
}
