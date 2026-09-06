use std::cmp::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use onlinerpg_shared::{inventory::GroundItem, inventory::ItemInstance, Monster, Player};
use serde::Deserialize;
use serde_json::{json, Map, Value};
use tokio::sync::Mutex;
use tracing::{info, warn};

use super::super::action::AgentAction;
use super::super::execute::handle_response;
use super::super::{decline_lapsed_trade, request_respawn, respawn_due};
use super::fighter;
use super::WorkerConfig;
use crate::state::SharedState;

#[derive(Debug, Deserialize)]
pub(super) struct Template {
    format: String,
    version: u32,
    #[serde(default)]
    rules: Vec<Rule>,
}

#[derive(Debug, Deserialize)]
struct Rule {
    #[allow(dead_code)]
    id: String,
    #[serde(default)]
    priority: i64,
    condition: Value,
    actions: Vec<Value>,
}

#[derive(Clone, Copy, Default)]
struct Context<'a> {
    monster: Option<&'a Monster>,
    item: Option<&'a ItemInstance>,
    player: Option<&'a Player>,
    ground_item: Option<&'a GroundItem>,
}

impl Template {
    pub(super) fn load(path: &str) -> Result<Self> {
        Self::parse(&std::fs::read_to_string(path)?)
    }

    pub(super) fn parse(json: &str) -> Result<Self> {
        let mut template: Self = serde_json::from_str(json)?;
        if template.format != "openmmo-worker" {
            bail!("unsupported worker format '{}'", template.format);
        }
        if template.version != 1 {
            bail!("unsupported worker version {}", template.version);
        }
        for rule in &template.rules {
            for action in &rule.actions {
                validate_action(action)?;
            }
        }
        template.rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        Ok(template)
    }

    pub(super) fn decide(&self, state: &SharedState) -> Result<Vec<Value>> {
        let Some(rule) = self
            .rules
            .iter()
            .find(|rule| condition(&rule.condition, state, Context::default()))
        else {
            return Ok(Vec::new());
        };
        rule.actions
            .iter()
            .take(8)
            .map(|action| canonical_action(action, state))
            .collect()
    }
}

fn validate_action(action: &Value) -> Result<()> {
    let mut object = action
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("action must be an object"))?;
    let kind = object
        .remove("action")
        .or_else(|| object.get("type").cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| anyhow::anyhow!("action type is required"))?;
    object.insert("type".to_string(), Value::from(kind.clone()));
    if kind == "attack" && object.get("monster_id").is_none() {
        let selector = object
            .remove("target")
            .ok_or_else(|| anyhow::anyhow!("attack target is required"))?;
        if selector.get("select").and_then(Value::as_str) != Some("monsters") {
            bail!("attack target must select monsters");
        }
        object.insert("monster_id".to_string(), Value::from("validation"));
    }
    serde_json::from_value::<AgentAction>(Value::Object(object))
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("invalid action '{kind}': {error}"))
}

fn condition(value: &Value, state: &SharedState, context: Context<'_>) -> bool {
    if let Some(value) = value.as_bool() {
        return value;
    }
    let Some(object) = value.as_object().filter(|object| object.len() == 1) else {
        return false;
    };
    if let Some(values) = object.get("all").and_then(Value::as_array) {
        return values.iter().all(|v| condition(v, state, context));
    }
    if let Some(values) = object.get("any").and_then(Value::as_array) {
        return values.iter().any(|v| condition(v, state, context));
    }
    if let Some(inner) = object.get("not") {
        return !condition(inner, state, context);
    }
    if let Some(selector) = object.get("exists") {
        return select(selector, state)
            .iter()
            .any(|candidate| selector_where(selector, state, *candidate));
    }
    for operator in ["eq", "ne", "lt", "lte", "gt", "gte"] {
        let Some(values) = object.get(operator).and_then(Value::as_array) else {
            continue;
        };
        if values.len() != 2 {
            return false;
        }
        let left = resolve(&values[0], state, context);
        let right = resolve(&values[1], state, context);
        return compare(operator, &left, &right);
    }
    false
}

fn compare(operator: &str, left: &Value, right: &Value) -> bool {
    match operator {
        "eq" => left == right,
        "ne" => left != right,
        "lt" | "lte" | "gt" | "gte" => {
            let Some((left, right)) = left.as_f64().zip(right.as_f64()) else {
                return false;
            };
            matches!(
                (operator, left.partial_cmp(&right)),
                ("lt", Some(Ordering::Less))
                    | ("lte", Some(Ordering::Less | Ordering::Equal))
                    | ("gt", Some(Ordering::Greater))
                    | ("gte", Some(Ordering::Greater | Ordering::Equal))
            )
        }
        _ => false,
    }
}

fn resolve(value: &Value, state: &SharedState, context: Context<'_>) -> Value {
    let Some(reference) = value.get("ref").and_then(Value::as_str) else {
        return value.clone();
    };
    let me = state.self_player.as_ref();
    let pct = |health: u32, maximum: u32| {
        Value::from(if maximum == 0 {
            0
        } else {
            health.saturating_mul(100) / maximum
        })
    };
    match reference {
        "self.health_pct" => me.map_or(Value::Null, |p| pct(p.health, p.max_health)),
        "self.level" => me.map_or(Value::Null, |p| Value::from(p.level)),
        "self.gold" => state.self_gold.map_or(Value::Null, Value::from),
        "monster.id" => context
            .monster
            .map_or(Value::Null, |m| Value::from(m.id.clone())),
        "monster.kind" | "monster.name" => context
            .monster
            .map_or(Value::Null, |m| Value::from(m.monster_type.clone())),
        "monster.level" => context
            .monster
            .map_or(Value::Null, |m| Value::from(fighter::monster_level(m))),
        "monster.distance" => context
            .monster
            .and_then(|m| me.map(|p| p.position.dist_xz_sq(&m.position).sqrt()))
            .map_or(Value::Null, Value::from),
        "monster.health_pct" => context
            .monster
            .map_or(Value::Null, |m| pct(m.health, m.max_health)),
        "item.id" | "item.name" => context
            .item
            .map_or(Value::Null, |i| Value::from(i.item_def_id.clone())),
        "item.count" | "item.quantity" => context
            .item
            .map_or(Value::Null, |i| Value::from(i.quantity)),
        "player.id" => context
            .player
            .map_or(Value::Null, |p| Value::from(p.id.get())),
        "player.name" => context
            .player
            .map_or(Value::Null, |p| Value::from(p.name.clone())),
        "player.distance" => context
            .player
            .and_then(|other| me.map(|p| p.position.dist_xz_sq(&other.position).sqrt()))
            .map_or(Value::Null, Value::from),
        "ground_item.id" => context
            .ground_item
            .map_or(Value::Null, |i| Value::from(i.instance_id)),
        "ground_item.item" | "ground_item.name" => context
            .ground_item
            .map_or(Value::Null, |i| Value::from(i.item_def_id.clone())),
        "ground_item.distance" => context
            .ground_item
            .and_then(|item| me.map(|p| p.position.dist_xz_sq(&item.position).sqrt()))
            .map_or(Value::Null, Value::from),
        _ => Value::Null,
    }
}

fn select<'a>(selector: &Value, state: &'a SharedState) -> Vec<Context<'a>> {
    match selector.get("select").and_then(Value::as_str) {
        Some("monsters") => state
            .nearby_monsters
            .values()
            .map(|monster| Context {
                monster: Some(monster),
                ..Context::default()
            })
            .collect(),
        Some("bag") => state
            .self_bag
            .iter()
            .map(|item| Context {
                item: Some(item),
                ..Context::default()
            })
            .collect(),
        Some("players") => state
            .nearby_players
            .values()
            .map(|player| Context {
                player: Some(player),
                ..Context::default()
            })
            .collect(),
        Some("ground_items") => state
            .ground_items_in_sight()
            .into_iter()
            .map(|(_, item)| Context {
                ground_item: Some(item),
                ..Context::default()
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn selector_where(selector: &Value, state: &SharedState, context: Context<'_>) -> bool {
    selector
        .get("where")
        .is_none_or(|where_| condition(where_, state, context))
}

fn canonical_action(action: &Value, state: &SharedState) -> Result<Value> {
    let mut object: Map<String, Value> = action
        .as_object()
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("action must be an object"))?;
    let kind = object
        .remove("action")
        .or_else(|| object.get("type").cloned())
        .and_then(|value| value.as_str().map(str::to_owned))
        .ok_or_else(|| anyhow::anyhow!("action type is required"))?;
    object.insert("type".to_string(), Value::String(kind.clone()));
    if kind == "attack" && !object.contains_key("monster_id") {
        let selector = object
            .remove("target")
            .ok_or_else(|| anyhow::anyhow!("attack target is required"))?;
        let target = select(&selector, state)
            .into_iter()
            .filter(|candidate| candidate.monster.is_some())
            .filter(|candidate| selector_where(&selector, state, *candidate))
            .min_by(|a, b| {
                let distance = |context: &Context<'_>| {
                    context.monster.and_then(|monster| {
                        state
                            .self_player
                            .as_ref()
                            .map(|me| me.position.dist_xz_sq(&monster.position))
                    })
                };
                distance(a)
                    .partial_cmp(&distance(b))
                    .unwrap_or(Ordering::Equal)
            })
            .and_then(|context| context.monster)
            .ok_or_else(|| anyhow::anyhow!("attack selector matched no monster"))?;
        object.insert("monster_id".to_string(), Value::from(target.id.clone()));
    }
    Ok(Value::Object(object))
}

/// Run a template document until the connection drops. Reuses the fighter's
/// housekeeping (trade decline, respawn) and the shared action executor; the
/// document itself is a pure `state -> actions` function evaluated above.
pub(super) async fn template_worker_driver(
    state: Arc<Mutex<SharedState>>,
    cfg: WorkerConfig,
    label: String,
    _api_base_url: String,
    watch: Option<Arc<crate::watch::NpcWatch>>,
) {
    let Some(path) = cfg.template_file.as_deref() else {
        warn!("[{label}] Template worker has no template_file");
        return;
    };
    let template = match Template::load(path) {
        Ok(template) => template,
        Err(error) => {
            warn!("[{label}] Template worker failed to load {path}: {error}");
            return;
        }
    };

    while !state.lock().await.in_game {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    info!("[{label}] Template worker in game, ready.");

    let mut dead_since: Option<Instant> = None;
    let mut last_turn = String::new();

    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;

        decline_lapsed_trade(&state, &label).await;

        let self_dead = {
            let mut s = state.lock().await;
            s.drain_events();
            s.drain_agent_events();
            s.in_game && s.self_player.as_ref().is_some_and(|p| p.health == 0)
        };

        if respawn_due(self_dead, &mut dead_since, Instant::now()) {
            request_respawn(&state, &None, &label).await;
        }
        if self_dead {
            continue;
        }

        let actions = {
            let s = state.lock().await;
            match template.decide(&s) {
                Ok(actions) => actions,
                Err(error) => {
                    warn!("[{label}] Template worker decision failed: {error}");
                    continue;
                }
            }
        };

        let actions = if actions.is_empty() {
            vec![json!({"type": "wait"})]
        } else {
            actions
        };
        let turn = json!({ "actions": actions }).to_string();
        if let Some(watch) = &watch {
            if last_turn != turn {
                watch.push("worker", turn.clone());
                last_turn.clone_from(&turn);
            }
        }

        handle_response(&state, &turn, &None, &None, false).await;
    }
}
