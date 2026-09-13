use super::*;

/// The floor we declare must follow our height: the server derives the
/// floor it collides against from the Y we send and validates the
/// declaration against it, so the two have to resolve identically.
#[test]
fn declared_floor_tracks_height() {
    let (s, dungeon, _rx) = dungeon_state();
    let (x, z) = (dungeon.entrance.x, dungeon.entrance.z);

    assert_eq!(s.wire_floor_at(x, z, dungeon.entrance.y), 0);
    assert_eq!(s.wire_floor_at(x, z, dungeon.floor_y(1)), -1);
    assert_eq!(s.wire_floor_at(x, z, dungeon.floor_y(3)), -3);
    // Mid-ramp resolves to whichever floor is nearer, never past the last.
    assert_eq!(s.wire_floor_at(x, z, dungeon.entrance.y - 1.0), 0);
    assert_eq!(s.wire_floor_at(x, z, dungeon.entrance.y - 3.0), -1);
    let deepest = dungeon.max_depth();
    assert_eq!(
        s.wire_floor_at(x, z, dungeon.floor_y(deepest) - 50.0),
        -(deepest as i8)
    );
}

/// Chest sightings run off the live passability, so the cell they tell the
/// mover to stand on must be one A* can actually route to — a clutter prop
/// is a sealed pillar, and aiming at it strands the agent every time.
#[test]
fn a_sighted_chest_is_approached_from_a_cell_a_path_can_reach() {
    let (mut s, dungeon, _rx) = cluttered_dungeon_state();
    let depth = in_the_chest_room(&mut s, &dungeon);
    let floor = dungeon.passability_floor(depth);

    let chests = s.chests_in_sight();
    assert!(
        chests
            .iter()
            .any(|c| c.kind == crate::dungeon::ChestKind::Treasure),
        "the chest room should show its treasure chest"
    );
    assert!(
        chests.len() > 1,
        "the chest room also holds a clutter chest"
    );
    for chest in chests {
        let a = chest.approach;
        assert!(
            s.world_cache.read().unwrap().is_walkable(a.x, a.z, floor),
            "{:?} is approached from a sealed cell",
            chest.kind
        );
        assert!(
            s.find_path_to(a.x, a.z, floor).found,
            "{:?} has no route to its approach cell",
            chest.kind
        );
    }
}

/// Every coordinate the underground state line hands the LLM has to be a
/// cell it can actually stand on. A shaft is walkable on this floor only
/// along one row — its min corner and the cell half a metre over are both
/// rock — so a wrong end or a rounded centre reads as a wall.
#[test]
fn the_floor_map_only_names_cells_the_agent_can_stand_on() {
    let (mut s, _crypt, _rx) = dungeon_state();
    let mut orientations = std::collections::HashSet::new();

    for def in onlinerpg_shared::dungeon::entrances() {
        let dungeon = s
            .world_cache
            .read()
            .unwrap()
            .dungeon_by_id(&def.id)
            .expect("registered dungeon");

        // Every door open: a shut one is a detour the mover handles, so it
        // must not be confused with a cell walled off for good.
        let doors: Vec<(u8, u32)> = (1..=dungeon.max_depth())
            .flat_map(|d| {
                dungeon
                    .closed_doors(d, &HashSet::new())
                    .into_iter()
                    .map(move |door| (d, door.door_id))
            })
            .collect();
        s.world_cache
            .write()
            .unwrap()
            .set_dungeon_doors(&dungeon.id, &doors);

        for depth in 1..=dungeon.max_depth() {
            let layout = &dungeon.layouts()[depth as usize - 1];
            orientations.insert(layout.up_shaft.reversed);
            let floor = dungeon.passability_floor(depth);
            stand_at(&mut s, &dungeon, depth, layout.rooms[0].center());

            let line = s.format_dungeon_state().expect("underground state line");
            let where_ = format!("{} floor {depth}", dungeon.id);
            let named = coordinates_in(&line);
            assert!(
                named.len() > layout.rooms.len(),
                "{where_} should name every room plus the stairs, got {named:?}"
            );
            for (x, z) in named {
                let p = Position { x, y: 0.0, z };
                // Printed coordinates must survive the round trip back to
                // the cell they name. Cell centres sit on .5, so rounding
                // them to whole metres silently names the cell next door.
                let cell = world_to_cell(&dungeon.entrance, x, z);
                let centre = cell_center(&dungeon.entrance, depth, cell);
                assert_eq!(
                    (centre.x, centre.z),
                    (x, z),
                    "{where_} prints ({x}, {z}), which reads back as the cell \
                         centred on ({}, {})\n{line}",
                    centre.x,
                    centre.z
                );
                assert!(
                    s.world_cache.read().unwrap().is_walkable(p.x, p.z, floor),
                    "{where_} points the agent at ({x}, {z}), which is solid rock\n{line}"
                );
                // A shaft's interior is carved but walled off from this
                // floor, so walkable is not enough — the goal has to be
                // routable too.
                assert!(
                    s.find_path_to(x, z, floor).found,
                    "{where_} points the agent at ({x}, {z}), which no route \
                         reaches\n{line}"
                );
            }
        }
    }

    assert_eq!(
        orientations.len(),
        2,
        "sample covers only one shaft orientation, so it cannot catch an \
             entry/exit mix-up"
    );
}

/// Breakables are offered off the live passability the same way chests
/// are, and a smashed one drops out of the listing.
#[tokio::test]
async fn a_smashed_prop_stops_being_offered() {
    let (mut s, dungeon, _rx) = cluttered_dungeon_state();
    let depth = in_the_chest_room(&mut s, &dungeon);
    let floor = dungeon.passability_floor(depth);
    let prop = s
        .breakables_in_sight()
        .first()
        .copied()
        .expect("the chest room holds breakable clutter");
    assert!(
        s.find_path_to(prop.approach.x, prop.approach.z, floor)
            .found,
        "a prop is offered with a cell we can route to"
    );

    s.push_event(ServerMessage::DungeonPropBroken {
        entrance_id: dungeon.id.clone(),
        depth,
        prop_id: prop.prop_id,
    });
    assert!(!s
        .breakables_in_sight()
        .iter()
        .any(|b| b.prop_id == prop.prop_id));
}

/// A clutter prop is marked opened before the server answers, because an
/// already-claimed one answers with silence. A rejection says it never
/// opened, so the mark has to come back off — otherwise a chest the agent
/// merely stood too far from is invisible for the rest of the floor.
#[tokio::test]
async fn a_rejected_prop_open_becomes_visible_again() {
    let (mut s, dungeon, _rx) = cluttered_dungeon_state();
    let depth = in_the_chest_room(&mut s, &dungeon);
    let prop = match s
        .chests_in_sight()
        .into_iter()
        .find(|c| matches!(c.kind, crate::dungeon::ChestKind::Prop(_)))
        .expect("the chest room holds a clutter chest")
        .kind
    {
        crate::dungeon::ChestKind::Prop(id) => id,
        _ => unreachable!(),
    };

    s.chest_open_sent(&dungeon.id, depth, crate::dungeon::ChestKind::Prop(prop));
    assert!(
        !s.chests_in_sight()
            .iter()
            .any(|c| c.kind == crate::dungeon::ChestKind::Prop(prop)),
        "a sent open hides the chest so we stop targeting it"
    );

    s.push_event(ServerMessage::InteractionRejected {
        reason: "Too far from the chest".to_string(),
    });
    assert!(
        s.chests_in_sight()
            .iter()
            .any(|c| c.kind == crate::dungeon::ChestKind::Prop(prop)),
        "a refused open leaves the chest there to try again"
    );
}

/// An emptied treasure chest still stands there, so it keeps showing — but
/// the line says it has nothing left, or the agent walks back to it all
/// night for the same refusal.
#[tokio::test]
async fn an_emptied_treasure_chest_says_so() {
    let (mut s, dungeon, _rx) = dungeon_state();
    let depth = in_the_chest_room(&mut s, &dungeon);
    assert!(!s.format_world_state().contains("refills at nightfall"));

    s.chest_open_sent(&dungeon.id, depth, crate::dungeon::ChestKind::Treasure);
    s.push_event(ServerMessage::InteractionRejected {
        reason: "The chest is empty (it refills at nightfall)".to_string(),
    });

    let world = s.format_world_state();
    assert!(
        world.contains("a great chest standing alone")
            && world.contains("you emptied it; it refills at nightfall"),
        "{world}"
    );
}

/// Registering the dungeon is all the shared A* needs to walk the entrance
/// stairwell: a path from the surface to floor 1 must exist and end there.
#[test]
fn a_path_leads_from_the_entrance_down_to_the_first_floor() {
    let (s, dungeon, _rx) = dungeon_state();
    let landing = dungeon.arrival_position(1).unwrap();
    let floor = dungeon.passability_floor(1);

    let path = s.find_path_to(landing.x, landing.z, floor);

    assert!(path.found, "no route from the entrance down to floor 1");
    assert_eq!(path.waypoints.last().map(|w| w.floor), Some(floor));
}

/// Every step of that descent must declare a floor the server accepts and
/// collides against identically — it derives the floor from the Y we send,
/// so a step whose declaration and height disagree gets snapped back.
#[test]
fn descending_steps_declare_a_floor_the_server_accepts() {
    let (mut s, dungeon, _rx) = dungeon_state();
    let landing = dungeon.arrival_position(1).unwrap();
    let path = s.find_path_to(landing.x, landing.z, dungeon.passability_floor(1));
    assert!(path.found);

    let mut seen_underground = false;
    for wp in &path.waypoints {
        // Mirror the mover: subdivide the leg and pose each step.
        loop {
            let position = s.self_player.as_ref().unwrap().position;
            let to_wp = crate::geom::PlanarDelta::to_xz(&position, wp.x, wp.z);
            if to_wp.dist < 0.1 {
                break;
            }
            let (sx, sz) = if to_wp.dist <= 3.0 {
                (wp.x, wp.z)
            } else {
                let r = 3.0 / to_wp.dist;
                (position.x + to_wp.dx * r, position.z + to_wp.dz * r)
            };
            let (pose, floor_level) = s.step_pose(sx, sz, wp.floor, position.y);
            if floor_level < 0 {
                seen_underground = true;
                let expected = dungeon.floor_y(floor_level.unsigned_abs());
                assert!(
                    (pose.y - expected).abs() <= FLOOR_Y_SANITY,
                    "floor {floor_level} declared at y={} (floor sits at {expected})",
                    pose.y
                );
            }
            s.self_player.as_mut().unwrap().position = pose;
            s.self_floor_level = floor_level;
        }
    }

    assert!(seen_underground, "the walk never went underground");
    assert_eq!(s.self_floor_level, -1);
}

/// Re-pathing from halfway down the stairs (after a fight or a correction)
/// must still work: those cells are keyed to the floor above, so searching
/// under the floor we are nearest would strand the agent on the steps.
#[test]
fn a_path_still_leads_on_from_halfway_down_the_stairs() {
    let (mut s, dungeon, _rx) = dungeon_state();
    let (x, z, y) = mid_shaft_point(&dungeon);
    s.self_player.as_mut().unwrap().position = Position { x, y, z };
    s.self_floor_level = s.wire_floor_at(x, z, y);

    assert_eq!(s.self_floor_level, -1, "mid-ramp should read as floor 1");
    assert_eq!(
        s.passability_floor(),
        0,
        "stair cells are keyed one floor up"
    );

    let landing = dungeon.arrival_position(1).unwrap();
    let path = s.find_path_to(landing.x, landing.z, dungeon.passability_floor(1));
    assert!(path.found, "no route on from the middle of the stairs");
}

/// The stairs down sit behind shut doors on most floors, so opening one has
/// to reopen the cells A* walks — otherwise the agent never gets past
/// floor 1 no matter how many doors it toggles.
#[test]
fn opening_a_door_reopens_the_route_behind_it() {
    let (mut s, dungeon, _rx) = dungeon_state();
    let landing = dungeon.arrival_position(1).unwrap();
    s.self_player.as_mut().unwrap().position = landing;
    s.self_floor_level = -1;

    let below = dungeon.arrival_position(2).unwrap();
    let goal_floor = dungeon.passability_floor(2);
    assert!(
        !s.find_path_to(below.x, below.z, goal_floor).found,
        "floor 1's stairs down are supposed to start sealed"
    );

    let doors: Vec<(u8, u32)> = dungeon
        .closed_doors(1, &HashSet::new())
        .iter()
        .map(|d| (1u8, d.door_id))
        .collect();
    assert!(!doors.is_empty());
    s.world_cache
        .write()
        .unwrap()
        .set_dungeon_doors(&dungeon.id, &doors);

    assert!(
        s.find_path_to(below.x, below.z, goal_floor).found,
        "the way down stayed sealed after opening floor 1's doors"
    );
}

/// The chest's one-open-per-character and the dungeon reset both key off the
/// server's `night_epoch`, and only players underground when it turns are
/// sent `DungeonReset`. Mirroring the epoch off the clock is what lets a
/// worker waiting on the surface know the chest has refilled.
#[test]
fn nightfall_refills_the_chest_for_anyone_watching_the_clock() {
    use onlinerpg_shared::celestial::get_solar_daylight_window;
    use onlinerpg_shared::GameDateTime;

    let (mut s, dungeon, _rx) = dungeon_state();
    let me = PlayerId::from(1);
    s.self_player_id = Some(me);

    let at = |hour: u8| GameDateTime {
        year: 1,
        month: 6,
        day: 10,
        hour,
        minute: 0,
    };
    // Whole hours either side of the boundary, since the epoch turns on the
    // fractional sunset hour and the clock we send reads in whole ones.
    let sunset = get_solar_daylight_window(6, 10).sunset_hour;
    assert!(
        (1.0..23.0).contains(&sunset),
        "this date needs a sunset inside the day to straddle"
    );
    let (before, after) = (sunset.floor() as u8 - 1, sunset.ceil() as u8);

    let sync = |s: &mut SharedState, hour: u8| {
        s.push_event(ServerMessage::GameTimeSync {
            datetime: at(hour),
            is_night: f64::from(hour) >= sunset,
        });
    };

    sync(&mut s, before);
    s.push_event(ServerMessage::DungeonChestOpened {
        entrance_id: dungeon.id.clone(),
        player_id: me,
        item_def_ids: Vec::new(),
        gold: 0,
    });
    assert!(s.treasure_chest_spent(&dungeon.id));

    // Same side of sunset: the chest still owes us nothing.
    sync(&mut s, before);
    assert!(s.treasure_chest_spent(&dungeon.id));

    sync(&mut s, after);
    assert!(
        !s.treasure_chest_spent(&dungeon.id),
        "sunset turned the epoch, so the chest has refilled"
    );
}

/// The server's own word for the same thing, for a worker that was still
/// underground when the sweep came through.
#[test]
fn the_reset_message_refills_the_chest_too() {
    let (mut s, dungeon, _rx) = dungeon_state();
    let me = PlayerId::from(1);
    s.self_player_id = Some(me);
    s.push_event(ServerMessage::DungeonChestOpened {
        entrance_id: dungeon.id.clone(),
        player_id: me,
        item_def_ids: Vec::new(),
        gold: 0,
    });
    assert!(s.treasure_chest_spent(&dungeon.id));

    s.push_event(ServerMessage::DungeonReset);
    assert!(!s.treasure_chest_spent(&dungeon.id));
}

/// Doors start shut, so a descent needing one opened is the normal case, and
/// the door can be most of a floor from where the leg starts. A search radius
/// shorter than the floor leaves the walker giving up on a door it never
/// looked at — and nothing about that attempt changes, so every retry fails
/// identically.
#[test]
fn every_descent_can_reach_the_door_that_blocks_it() {
    use onlinerpg_shared::dungeon::{interior_doors, world_to_cell};

    let reach = crate::driver::walk::door_search_dist(true);
    for (x, z) in [
        (-1450.0f32, 4720.0f32),
        (-1616.0, 4918.0),
        (-1785.2, 5072.3),
    ] {
        let (mut s, dungeon, _rx) = dungeon_state_at(x, z);
        for depth in 2..=dungeon.max_depth() {
            // The landing a descent from above actually arrives on.
            let up = dungeon.arrival_position(depth - 1).unwrap();
            let cell = world_to_cell(&dungeon.entrance, up.x, up.z);
            let from = stand_at(&mut s, &dungeon, depth - 1, cell);

            let goal = dungeon.arrival_position(depth).unwrap();
            let floor = dungeon.passability_floor(depth);
            let all: Vec<(u8, u32)> = interior_doors(&dungeon.layouts()[(depth - 2) as usize])
                .into_iter()
                .map(|door| (depth - 1, door.door_id))
                .collect();

            // Doors on the floor we stand on are the whole story: opening
            // them opens the way down.
            s.world_cache
                .write()
                .unwrap()
                .set_dungeon_doors(&dungeon.id, &all);
            assert!(
                s.find_path_to(goal.x, goal.z, floor).found,
                "{} {} -> {depth}: unreachable even with this floor's doors open",
                dungeon.name,
                depth - 1
            );

            // And each of them is inside the radius the walker searches, so
            // none can be the one it never tries.
            s.world_cache
                .write()
                .unwrap()
                .set_dungeon_doors(&dungeon.id, &[]);
            for door in dungeon.closed_doors(depth - 1, &std::collections::HashSet::new()) {
                let near = door
                    .sides
                    .iter()
                    .map(|(dx, dz)| (dx - from.x).hypot(dz - from.z))
                    .fold(f32::INFINITY, f32::min);
                assert!(
                    near <= reach,
                    "{} floor {}: door {} is {near:.0}m from the landing, past the {reach:.0}m \
                     the walker searches",
                    dungeon.name,
                    depth - 1,
                    door.door_id
                );
            }
        }
    }
}

/// The chest is a 1x1 collision pillar, so its own cell is a goal no path can
/// arrive at. Walking at it stopped short of the chamber in every dungeon
/// whose layout did not happen to drop the partial path inside the room —
/// which read from the outside as a chest the worker simply never opened.
#[test]
fn the_chest_is_approached_from_a_cell_a_path_can_reach() {
    for (x, z) in [
        (-1450.0f32, 4720.0f32),
        (-1616.0, 4918.0),
        (-1785.2, 5072.3),
    ] {
        let (mut s, dungeon, _rx) = dungeon_state_at(x, z);
        let depth = dungeon.max_depth();
        let chest = dungeon
            .treasure_position()
            .expect("a chest on the last floor");
        let spot = dungeon.treasure_approach().expect("and a way up to it");
        assert_ne!(
            (spot.x, spot.z),
            (chest.x, chest.z),
            "{}: the chest's own cell is sealed",
            dungeon.name
        );

        let floor = dungeon.passability_floor(depth);
        assert!(
            !s.world_cache
                .read()
                .unwrap()
                .is_walkable(chest.x, chest.z, floor),
            "{}: the chest cell would be walkable, so this test proves nothing",
            dungeon.name
        );
        assert!(
            s.world_cache
                .read()
                .unwrap()
                .is_walkable(spot.x, spot.z, floor),
            "{}: the approach cell is not standable",
            dungeon.name
        );
        // And it is beside the chest, not merely somewhere on the floor: the
        // sighting that turns the walk into an open is room-scoped.
        stand_at(
            &mut s,
            &dungeon,
            depth,
            onlinerpg_shared::dungeon::world_to_cell(&dungeon.entrance, spot.x, spot.z),
        );
        assert!(
            s.chests_in_sight()
                .iter()
                .any(|c| c.kind == crate::dungeon::ChestKind::Treasure),
            "{}: standing on the approach must put the chest in sight",
            dungeon.name
        );
    }
}
