//! Narrow chart-stack synchronization helpers for Manual chart trading.

use std::collections::{BTreeMap, HashSet};

use moon_core::feed::{NewStrategySpec, StrategyRow};
use moon_core::session::CoreId;

use crate::Backend;
use crate::backend::manual_trading::{
    FIELD_USE_HOOK_STRATEGY, HOOK_STRATEGY_KIND, MANUAL_STRATEGY_KIND, strat_field_value,
};

/// Strategy compatibility for one chart inside an active manual-order SYNC frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SyncManualStrategyStatus {
    /// A Manual strategy with the selected name exists and carries matching order parameters.
    Ready,
    /// The target core has no Manual strategy with the selected name.
    MissingStrategy { strategy_name: String },
    /// The name exists, but the target core's parameters differ from the anchor's.
    DifferentParams {
        strategy_name: String,
        diffs: Vec<SyncManualStrategyDiff>,
    },
}

/// One parameter mismatch between the anchor Manual strategy and a target core's strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualStrategyDiff {
    pub(crate) field: String,
    pub(crate) anchor: String,
    pub(crate) target: String,
}

/// Aggregate SYNC health across the selected comparison stack.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SyncManualIssueSummary {
    pub(crate) missing: usize,
    pub(crate) different: usize,
}

impl SyncManualIssueSummary {
    pub(crate) fn is_empty(&self) -> bool {
        self.missing == 0 && self.different == 0
    }

    pub(crate) fn count(&self) -> usize {
        self.missing + self.different
    }

    pub(crate) fn tooltip(&self) -> String {
        let mut lines = vec!["Диагностика SYNC".to_string()];
        if self.missing > 0 {
            lines.push(format!("Нет ручной стратегии: {} ядр.", self.missing));
        }
        if self.different > 0 {
            lines.push(format!("Отличаются параметры: {} ядр.", self.different));
        }
        lines.push("Нажмите, чтобы посмотреть и исправить выбранные ядра.".to_string());
        lines.join("\n")
    }
}

/// One source strategy that can be copied into cores where its name is absent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualRepairStrategySpec {
    name: String,
    kind_ordinal: u8,
    folder_path: String,
    fields: Vec<(String, String)>,
}

impl SyncManualRepairStrategySpec {
    fn from_row(row: &StrategyRow) -> Self {
        Self {
            name: row.name.trim().to_string(),
            kind_ordinal: row.kind_ordinal,
            folder_path: row.folder_path.clone(),
            fields: row.fields.clone(),
        }
    }

    fn label(&self) -> String {
        let kind = match self.kind_ordinal {
            MANUAL_STRATEGY_KIND => "Ручная",
            HOOK_STRATEGY_KIND => "Hook",
            _ => "Стратегия",
        };
        format!("{kind} {}", self.name)
    }

    fn to_new_strategy_spec(&self) -> NewStrategySpec {
        NewStrategySpec {
            kind_ordinal: self.kind_ordinal,
            folder_path: self.folder_path.clone(),
            fields: self.fields.clone(),
            insert_after: None,
        }
    }
}

/// Strategies that must be copied to one core before SYNC can order through the selected Manual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualRepairCorePlan {
    pub(crate) core: CoreId,
    pub(crate) server: String,
    pub(crate) missing: Vec<SyncManualRepairStrategySpec>,
}

/// Cross-core repair plan for one active Manual strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualRepairPlan {
    pub(crate) origin: CoreId,
    pub(crate) strategy_name: String,
    pub(crate) cores: Vec<SyncManualRepairCorePlan>,
}

impl SyncManualRepairPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.cores.iter().all(|core| core.missing.is_empty())
    }

    pub(crate) fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Скопировать недостающие стратегии для SYNC \"{}\"?",
            self.strategy_name
        )];
        for core in &self.cores {
            if core.missing.is_empty() {
                continue;
            }
            let missing = core
                .missing
                .iter()
                .map(SyncManualRepairStrategySpec::label)
                .collect::<Vec<_>>()
                .join(", ");
            lines.push(format!("{}: {}", core.server, missing));
        }
        lines.join("\n")
    }
}

/// One target field value chosen by strict majority among selected cores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualParamMajority {
    pub(crate) field: String,
    pub(crate) label: String,
    pub(crate) value: String,
    pub(crate) count: usize,
    pub(crate) total: usize,
}

/// One field edit needed to align a core with the majority value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualParamRepairChange {
    field: String,
    pub(crate) label: String,
    pub(crate) current_value: String,
    pub(crate) majority_value: String,
    pub(crate) majority_count: usize,
    pub(crate) total_count: usize,
}

/// Parameter edits for one core's Manual strategy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualParamRepairCorePlan {
    pub(crate) core: CoreId,
    pub(crate) server: String,
    strategy_id: u64,
    pub(crate) changes: Vec<SyncManualParamRepairChange>,
}

/// Cross-core plan that aligns the active Manual strategy with strict-majority parameters.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncManualParamRepairPlan {
    pub(crate) strategy_name: String,
    pub(crate) cores: Vec<SyncManualParamRepairCorePlan>,
}

impl SyncManualParamRepairPlan {
    pub(crate) fn is_empty(&self) -> bool {
        self.cores.iter().all(|core| core.changes.is_empty())
    }

    pub(crate) fn summary(&self) -> String {
        let mut lines = vec![format!(
            "Исправить отличия ручной стратегии \"{}\" по большинству ядер?",
            self.strategy_name
        )];
        for core in &self.cores {
            if core.changes.is_empty() {
                continue;
            }
            lines.push(format!("{}:", core.server));
            for change in &core.changes {
                lines.push(format!(
                    "  {}: {} -> {} (большинство {}/{})",
                    change.label,
                    change.current_value,
                    change.majority_value,
                    change.majority_count,
                    change.total_count,
                ));
            }
        }
        lines.join("\n")
    }
}

impl SyncManualStrategyStatus {
    /// Human-readable hover text for a SYNC diagnostic frame.
    pub(crate) fn tooltip(&self) -> Option<String> {
        match self {
            SyncManualStrategyStatus::Ready => None,
            SyncManualStrategyStatus::MissingStrategy { strategy_name } => Some(format!(
                "SYNC: на этом ядре нет ручной стратегии \"{strategy_name}\".\nНажмите индикатор рядом с SYNC, чтобы скопировать недостающее."
            )),
            SyncManualStrategyStatus::DifferentParams {
                strategy_name,
                diffs,
            } => {
                let mut lines = vec![format!(
                    "SYNC: у ручной стратегии \"{strategy_name}\" отличаются параметры:"
                )];
                for diff in diffs.iter().take(8) {
                    lines.push(format!(
                        "{}: ведущее={} / это={}",
                        human_strategy_field_label(&diff.field),
                        diff.anchor,
                        diff.target
                    ));
                }
                if diffs.len() > 8 {
                    lines.push(format!("и еще {}", diffs.len() - 8));
                }
                Some(lines.join("\n"))
            }
        }
    }
}

impl Backend {
    /// Return synchronized manual-order targets for the current comparison stack.
    pub(crate) fn sync_manual_order_targets(
        &mut self,
        origin: CoreId,
        market: &str,
        candidates: &[(CoreId, String)],
    ) -> Vec<(CoreId, String)> {
        let Some(origin_strategy) = self.active_manual_strategy_row(origin) else {
            log::warn!(
                "sync manual order blocked: origin core={} has no enabled manual strategy",
                moon_core::feed::core_label(origin)
            );
            return Vec::new();
        };
        let origin_strategy_name = origin_strategy.name.trim().to_string();
        let mut seen = HashSet::new();
        candidates
            .iter()
            .filter_map(|(core, candidate_market)| {
                if !seen.insert(*core) || candidate_market != market {
                    return None;
                }
                if !self.core_active(*core) {
                    return None;
                }
                match self.sync_manual_strategy_status(origin, *core) {
                    Some(SyncManualStrategyStatus::Ready) => {}
                    Some(SyncManualStrategyStatus::DifferentParams { .. }) => {
                        log::warn!(
                            "sync manual order warning: core={} uses Manual strategy {:?} with parameters different from the anchor; order will use that core's parameters",
                            moon_core::feed::core_label(*core),
                            origin_strategy_name
                        );
                    }
                    Some(SyncManualStrategyStatus::MissingStrategy { .. }) | None => {
                        log::warn!(
                            "sync manual order skipped: core={} has no Manual strategy named {:?}",
                            moon_core::feed::core_label(*core),
                            origin_strategy_name
                        );
                        return None;
                    }
                }
                if !self.ensure_manual_strategy_selected_by_name(*core, &origin_strategy_name) {
                    return None;
                }
                Some((*core, candidate_market.clone()))
            })
            .collect()
    }

    /// Compare the selected anchor Manual strategy with the named strategy on one target core.
    pub(crate) fn sync_manual_strategy_status(
        &self,
        origin: CoreId,
        target: CoreId,
    ) -> Option<SyncManualStrategyStatus> {
        let origin = self.active_manual_strategy_row(origin)?;
        let Some(target) = self.manual_strategy_by_name(target, &origin.name) else {
            return Some(SyncManualStrategyStatus::MissingStrategy {
                strategy_name: origin.name.trim().to_string(),
            });
        };
        let diffs = manual_strategy_diffs(origin, target);
        if diffs.is_empty() {
            Some(SyncManualStrategyStatus::Ready)
        } else {
            Some(SyncManualStrategyStatus::DifferentParams {
                strategy_name: origin.name.trim().to_string(),
                diffs,
            })
        }
    }

    /// Plan missing strategy copies needed for SYNC to use the anchor's active Manual strategy.
    pub(crate) fn sync_manual_repair_plan(
        &self,
        origin: CoreId,
        candidates: &[(CoreId, String)],
    ) -> Option<SyncManualRepairPlan> {
        let origin_strategy = self.active_manual_strategy_row(origin)?;
        let source_specs = self.sync_manual_repair_sources(origin, candidates);

        let mut seen = HashSet::new();
        let mut cores = Vec::new();
        for (target_core, _) in candidates {
            if !seen.insert(*target_core) || !self.core_active(*target_core) {
                continue;
            }
            let Some(target_data) = self.session.store().core(*target_core) else {
                continue;
            };
            let existing_names: HashSet<&str> = target_data
                .strategies
                .iter()
                .map(|strategy| strategy.name.trim())
                .filter(|name| !name.is_empty())
                .collect();
            let missing: Vec<SyncManualRepairStrategySpec> = source_specs
                .iter()
                .filter(|source| !existing_names.contains(source.name.as_str()))
                .cloned()
                .collect();
            if missing.is_empty() {
                continue;
            }
            cores.push(SyncManualRepairCorePlan {
                core: *target_core,
                server: self.core_display_name(*target_core),
                missing,
            });
        }
        Some(SyncManualRepairPlan {
            origin,
            strategy_name: origin_strategy.name.trim().to_string(),
            cores,
        })
        .filter(|plan| !plan.is_empty())
    }

    /// Copy only missing strategies from the anchor core to the target cores in a repair plan.
    pub(crate) fn apply_sync_manual_repair_plan(
        &self,
        plan: &SyncManualRepairPlan,
    ) -> anyhow::Result<()> {
        for core_plan in &plan.cores {
            let Some(target_data) = self.session.store().core(core_plan.core) else {
                continue;
            };
            let existing_names: HashSet<&str> = target_data
                .strategies
                .iter()
                .map(|strategy| strategy.name.trim())
                .filter(|name| !name.is_empty())
                .collect();
            let specs: Vec<NewStrategySpec> = core_plan
                .missing
                .iter()
                .filter(|missing| !existing_names.contains(missing.name.as_str()))
                .map(SyncManualRepairStrategySpec::to_new_strategy_spec)
                .collect();
            self.session.create_strategies(core_plan.core, specs)?;
        }
        Ok(())
    }

    /// Plan field edits that make each target's active Manual strategy match strict majority.
    pub(crate) fn sync_manual_param_repair_plan(
        &self,
        origin: CoreId,
        candidates: &[(CoreId, String)],
    ) -> Option<SyncManualParamRepairPlan> {
        let origin_strategy = self.active_manual_strategy_row(origin)?;
        let strategy_name = origin_strategy.name.trim().to_string();
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        let mut cores = Vec::with_capacity(candidates.len() + 1);
        cores.push(origin);
        cores.extend(candidates.iter().map(|(core, _)| *core));
        for core in cores {
            if !seen.insert(core) || !self.core_active(core) {
                continue;
            }
            let Some(strategy) = self.manual_strategy_by_name(core, &strategy_name) else {
                continue;
            };
            rows.push((core, self.core_display_name(core), strategy));
        }
        if rows.len() < 2 {
            return None;
        }

        let majority_fields = strict_majority_fields(origin, &rows);
        if majority_fields.is_empty() {
            return None;
        }

        let mut core_plans = Vec::new();
        for (core, server, strategy) in rows {
            let fields = normalized_strategy_field_values(&strategy.fields);
            let mut changes = Vec::new();
            for majority in majority_fields.values() {
                let current = fields.get(majority.field.as_str());
                let current_value = current
                    .map(|(_, value)| value.clone())
                    .unwrap_or_else(|| "<not set>".to_string());
                if current_value == majority.value {
                    continue;
                }
                let field = current
                    .map(|(field, _)| field.clone())
                    .unwrap_or_else(|| majority.field.clone());
                changes.push(SyncManualParamRepairChange {
                    field,
                    label: majority.label.clone(),
                    current_value,
                    majority_value: majority.value.clone(),
                    majority_count: majority.count,
                    total_count: majority.total,
                });
            }
            if changes.is_empty() {
                continue;
            }
            core_plans.push(SyncManualParamRepairCorePlan {
                core,
                server,
                strategy_id: strategy.id,
                changes,
            });
        }

        Some(SyncManualParamRepairPlan {
            strategy_name,
            cores: core_plans,
        })
        .filter(|plan| !plan.is_empty())
    }

    /// Apply strict-majority Manual parameter edits.
    pub(crate) fn apply_sync_manual_param_repair_plan(
        &self,
        plan: &SyncManualParamRepairPlan,
    ) -> anyhow::Result<()> {
        for core_plan in &plan.cores {
            let changes: Vec<(String, String)> = core_plan
                .changes
                .iter()
                .map(|change| (change.field.clone(), change.majority_value.clone()))
                .collect();
            let edits = vec![(core_plan.strategy_id, changes)];
            self.session.edit_strategies(core_plan.core, edits)?;
        }
        Ok(())
    }

    /// Return synchronized market-action targets for Cancel Buy / Panic Sell.
    pub(crate) fn sync_market_action_targets(
        &self,
        origin: CoreId,
        market: &str,
        candidates: &[(CoreId, String)],
    ) -> Vec<(CoreId, String)> {
        let mut seen = HashSet::new();
        let targets: Vec<(CoreId, String)> = candidates
            .iter()
            .filter_map(|(core, candidate_market)| {
                if !seen.insert(*core) || candidate_market != market {
                    return None;
                }
                if !self.core_active(*core) {
                    return None;
                }
                Some((*core, candidate_market.clone()))
            })
            .collect();
        if targets.is_empty() {
            vec![(origin, market.to_string())]
        } else {
            targets
        }
    }

    /// Store and send a manual-strategy selection, then mirror it inside the active chart SYNC
    /// context by exact Manual strategy name.
    pub(crate) fn set_manual_strategy_with_sync(&mut self, core: CoreId, on: bool, id: u64) -> u64 {
        let strategy_name = self
            .manual_strategy_name_by_id(core, id)
            .map(str::to_string);
        let stored_id = self.set_manual_strategy(core, on, id);
        let Some(strategy_name) = strategy_name else {
            return stored_id;
        };
        self.sync_manual_strategy_selection(core, on, &strategy_name);
        stored_id
    }

    /// Replace the runtime chart SYNC scope used by the header/manual-strategy picker.
    pub(crate) fn set_sync_manual_strategy_targets(&mut self, targets: Vec<(CoreId, String)>) {
        self.sync_manual_strategy_targets = targets;
    }

    /// Clear the runtime chart SYNC scope only if it still belongs to the stack that published it.
    pub(crate) fn clear_sync_manual_strategy_targets_if(&mut self, targets: &[(CoreId, String)]) {
        if self.sync_manual_strategy_targets == targets {
            self.sync_manual_strategy_targets.clear();
        }
    }

    fn sync_manual_strategy_selection(&mut self, origin: CoreId, on: bool, strategy_name: &str) {
        let targets = self.sync_manual_strategy_targets.clone();
        let mut seen = HashSet::new();
        for (target_core, _) in targets {
            if target_core == origin || !seen.insert(target_core) {
                continue;
            }
            if !self.core_active(target_core) {
                continue;
            }
            let Some(target_data) = self.session.store().core(target_core) else {
                continue;
            };
            if on {
                if let Some(target_id) = target_data
                    .strategies
                    .iter()
                    .find(|strategy| {
                        strategy.kind_ordinal == MANUAL_STRATEGY_KIND
                            && strategy.name == strategy_name
                    })
                    .map(|strategy| strategy.id)
                {
                    self.set_manual_strategy(target_core, true, target_id);
                }
            } else if self.active_manual_strategy_name(target_core) == Some(strategy_name) {
                let (_, target_id) = self.manual_strat_state(target_core);
                self.set_manual_strategy(target_core, false, target_id);
            }
        }
    }

    fn core_active(&self, core: CoreId) -> bool {
        self.config
            .servers
            .iter()
            .find(|server| server.id == core)
            .is_some_and(|server| server.active)
    }

    fn core_display_name(&self, core: CoreId) -> String {
        self.config
            .servers
            .iter()
            .find(|server| server.id == core)
            .map(|server| server.name.clone())
            .unwrap_or_else(|| moon_core::feed::core_label(core).to_string())
    }

    fn ensure_manual_strategy_selected_by_name(
        &mut self,
        core: CoreId,
        strategy_name: &str,
    ) -> bool {
        if self.active_manual_strategy_name(core) == Some(strategy_name) {
            return true;
        }
        let Some(strategy_id) = self.manual_strategy_id_by_name(core, strategy_name) else {
            log::warn!(
                "sync manual order skipped: core={} has no Manual strategy named {strategy_name:?}",
                moon_core::feed::core_label(core)
            );
            return false;
        };
        self.set_manual_strategy(core, true, strategy_id);
        true
    }

    fn active_manual_strategy_name(&self, core: CoreId) -> Option<&str> {
        self.active_manual_strategy_row(core)
            .map(|strategy| strategy.name.as_str())
    }

    fn manual_strategy_name_by_id(&self, core: CoreId, id: u64) -> Option<&str> {
        self.manual_strategy_by_id(core, id)
            .map(|strategy| strategy.name.as_str())
    }

    fn active_manual_strategy_row(&self, core: CoreId) -> Option<&StrategyRow> {
        let (on, id) = self.manual_strat_state(core);
        if !on || id == 0 {
            return None;
        }
        self.manual_strategy_by_id(core, id)
    }

    fn manual_strategy_by_id(&self, core: CoreId, id: u64) -> Option<&StrategyRow> {
        if id == 0 {
            return None;
        }
        self.session
            .store()
            .core(core)?
            .strategies
            .iter()
            .find(|strategy| strategy.id == id && strategy.kind_ordinal == MANUAL_STRATEGY_KIND)
    }

    fn manual_strategy_id_by_name(&self, core: CoreId, name: &str) -> Option<u64> {
        self.manual_strategy_by_name(core, name)
            .map(|strategy| strategy.id)
    }

    fn manual_strategy_by_name(&self, core: CoreId, name: &str) -> Option<&StrategyRow> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        self.session
            .store()
            .core(core)?
            .strategies
            .iter()
            .find(|strategy| {
                strategy.id != 0
                    && strategy.kind_ordinal == MANUAL_STRATEGY_KIND
                    && strategy.name.trim() == name
            })
    }

    fn sync_manual_repair_sources(
        &self,
        origin: CoreId,
        candidates: &[(CoreId, String)],
    ) -> Vec<SyncManualRepairStrategySpec> {
        let mut cores = Vec::with_capacity(candidates.len() + 1);
        cores.push(origin);
        cores.extend(candidates.iter().map(|(core, _)| *core));

        let mut seen_cores = HashSet::new();
        let mut seen_names = HashSet::new();
        let mut sources = Vec::new();
        for core in cores {
            if !seen_cores.insert(core) || !self.core_active(core) {
                continue;
            }
            let Some(data) = self.session.store().core(core) else {
                continue;
            };
            let schema = data.schema.as_ref();
            for manual in data.strategies.iter().filter(|strategy| {
                strategy.id != 0 && strategy.kind_ordinal == MANUAL_STRATEGY_KIND
            }) {
                let manual_name = manual.name.trim();
                if manual_name.is_empty() || seen_names.contains(manual_name) {
                    continue;
                }
                if let Some(hook_name) = linked_hook_name(manual, schema) {
                    if !seen_names.contains(hook_name.as_str()) {
                        if let Some(hook) = data.strategies.iter().find(|strategy| {
                            strategy.id != 0
                                && strategy.kind_ordinal == HOOK_STRATEGY_KIND
                                && strategy.name.trim() == hook_name
                        }) {
                            seen_names.insert(hook.name.trim().to_string());
                            sources.push(SyncManualRepairStrategySpec::from_row(hook));
                        } else {
                            log::warn!(
                                "sync manual repair warning: Manual strategy {:?} references missing hook strategy {:?} on core={}",
                                manual.name,
                                hook_name,
                                moon_core::feed::core_label(core),
                            );
                        }
                    }
                }
                seen_names.insert(manual_name.to_string());
                sources.push(SyncManualRepairStrategySpec::from_row(manual));
            }
        }
        sources
    }
}

fn linked_hook_name(
    manual: &StrategyRow,
    schema: Option<&moon_core::feed::StrategySchemaModel>,
) -> Option<String> {
    strat_field_value(manual, schema, FIELD_USE_HOOK_STRATEGY)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn manual_strategy_diffs(a: &StrategyRow, b: &StrategyRow) -> Vec<SyncManualStrategyDiff> {
    let mut diffs = Vec::new();
    if a.kind_ordinal != b.kind_ordinal {
        diffs.push(SyncManualStrategyDiff {
            field: "kind".to_string(),
            anchor: a.kind_ordinal.to_string(),
            target: b.kind_ordinal.to_string(),
        });
    }
    if a.is_short != b.is_short {
        diffs.push(SyncManualStrategyDiff {
            field: "side".to_string(),
            anchor: strategy_side(a.is_short).to_string(),
            target: strategy_side(b.is_short).to_string(),
        });
    }

    let a_fields = normalized_strategy_fields(&a.fields);
    let b_fields = normalized_strategy_fields(&b.fields);
    for (field, a_value) in &a_fields {
        let b_value = b_fields.get(field).copied();
        if Some(*a_value) == b_value {
            continue;
        }
        diffs.push(SyncManualStrategyDiff {
            field: (*field).to_string(),
            anchor: (*a_value).to_string(),
            target: b_value.unwrap_or("<missing>").to_string(),
        });
    }
    for (field, b_value) in &b_fields {
        if a_fields.contains_key(field) {
            continue;
        }
        diffs.push(SyncManualStrategyDiff {
            field: (*field).to_string(),
            anchor: "<missing>".to_string(),
            target: (*b_value).to_string(),
        });
    }
    diffs
}

fn normalized_strategy_fields(fields: &[(String, String)]) -> BTreeMap<&str, &str> {
    fields
        .iter()
        .filter(|(name, _)| !ignored_strategy_field(name))
        .map(|(name, value)| (name.trim(), value.trim()))
        .collect()
}

fn normalized_strategy_field_values(
    fields: &[(String, String)],
) -> BTreeMap<String, (String, String)> {
    fields
        .iter()
        .filter(|(name, _)| !ignored_strategy_field(name))
        .map(|(name, value)| {
            let field = name.trim().to_string();
            (field.clone(), (field, value.trim().to_string()))
        })
        .collect()
}

fn strict_majority_fields(
    origin: CoreId,
    rows: &[(CoreId, String, &StrategyRow)],
) -> BTreeMap<String, SyncManualParamMajority> {
    let mut votes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    let mut origin_values: BTreeMap<String, String> = BTreeMap::new();
    for (core, _, row) in rows {
        for (field, (_, value)) in normalized_strategy_field_values(&row.fields) {
            *votes
                .entry(field.clone())
                .or_default()
                .entry(value.clone())
                .or_default() += 1;
            if *core == origin {
                origin_values.insert(field, value);
            }
        }
    }

    let total = rows.len();
    let mut majorities = BTreeMap::new();
    for (field, counts) in votes {
        let mut best_value = String::new();
        let mut best_count = 0usize;
        for (value, count) in counts {
            if count > best_count
                || (count == best_count && origin_values.get(&field) == Some(&value))
            {
                best_value = value;
                best_count = count;
            }
        }
        if best_count * 2 <= total {
            continue;
        }
        majorities.insert(
            field.clone(),
            SyncManualParamMajority {
                label: human_strategy_field_label(&field),
                field,
                value: best_value,
                count: best_count,
                total,
            },
        );
    }
    majorities
}

fn ignored_strategy_field(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "id" | "name"
            | "folder_path"
            | "checked"
            | "comment"
            | "last edit date"
            | "lasteditdate"
            | "last_edit_date"
    )
}

fn human_strategy_field_label(field: &str) -> String {
    match field.trim() {
        FIELD_USE_HOOK_STRATEGY => "Ведущий hook".to_string(),
        "UseStopLoss" => "Стоп включен".to_string(),
        "StopLoss" => "Глубина стопа".to_string(),
        "UseTakeProfit" => "Тейк включен".to_string(),
        "TakeProfit" => "Глубина тейка".to_string(),
        "SellPrice" => "Глубина продажи".to_string(),
        "BuyPrice" => "Глубина выставления".to_string(),
        "OrderSize" | "OrderUsd" | "OrderUSDT" => "Размер ордера".to_string(),
        "Last Edit Date" => "Дата изменения".to_string(),
        "Comment" => "Комментарий".to_string(),
        other => humanize_strategy_field(other),
    }
}

fn humanize_strategy_field(field: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for ch in field.trim().replace(['_', '-'], " ").chars() {
        if ch.is_uppercase() && prev_lower {
            out.push(' ');
        }
        out.push(ch);
        prev_lower = ch.is_lowercase() || ch.is_ascii_digit();
    }
    let out = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() {
        field.to_string()
    } else {
        out
    }
}

fn strategy_side(is_short: bool) -> &'static str {
    if is_short { "short" } else { "long" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strategy(id: u64, kind_ordinal: u8, name: &str) -> StrategyRow {
        StrategyRow {
            id,
            name: name.to_string(),
            kind: name.to_string(),
            kind_ordinal,
            folder_path: "Manual".to_string(),
            checked: false,
            is_short: false,
            fields: Vec::new(),
        }
    }

    #[test]
    fn manual_diff_ignores_service_fields_but_reports_order_parameters() {
        let mut anchor = strategy(1, MANUAL_STRATEGY_KIND, "M1");
        anchor.fields = vec![
            ("Name".to_string(), "M1".to_string()),
            ("Last Edit Date".to_string(), "2026-04-14 02:55".to_string()),
            ("Comment".to_string(), "anchor note".to_string()),
            ("Depth".to_string(), "2".to_string()),
            ("OrderUsd".to_string(), "1000".to_string()),
        ];
        let mut target = strategy(2, MANUAL_STRATEGY_KIND, "M1");
        target.fields = vec![
            ("Name".to_string(), "M1".to_string()),
            ("Last Edit Date".to_string(), "2026-08-07 15:04".to_string()),
            ("Comment".to_string(), "target note".to_string()),
            ("Depth".to_string(), "4".to_string()),
            ("OrderUsd".to_string(), "1000".to_string()),
        ];

        let diffs = manual_strategy_diffs(&anchor, &target);

        assert_eq!(
            diffs,
            vec![SyncManualStrategyDiff {
                field: "Depth".to_string(),
                anchor: "2".to_string(),
                target: "4".to_string(),
            }]
        );
    }

    #[test]
    fn repair_summary_names_manual_and_hook_copies() {
        let manual =
            SyncManualRepairStrategySpec::from_row(&strategy(10, MANUAL_STRATEGY_KIND, "Manual A"));
        let hook =
            SyncManualRepairStrategySpec::from_row(&strategy(11, HOOK_STRATEGY_KIND, "Hook A"));
        let plan = SyncManualRepairPlan {
            origin: 1,
            strategy_name: "Manual A".to_string(),
            cores: vec![SyncManualRepairCorePlan {
                core: 2,
                server: "BGF-SUB1".to_string(),
                missing: vec![hook, manual],
            }],
        };

        assert_eq!(
            plan.summary(),
            "Скопировать недостающие стратегии для SYNC \"Manual A\"?\nBGF-SUB1: Hook Hook A, Manual Manual A"
        );
    }

    #[test]
    fn repair_spec_keeps_fields_for_core_creation() {
        let mut source = strategy(22, MANUAL_STRATEGY_KIND, "Manual B");
        source.folder_path = "Folder/Sub".to_string();
        source.fields = vec![("Depth".to_string(), "1.8".to_string())];

        let spec = SyncManualRepairStrategySpec::from_row(&source).to_new_strategy_spec();

        assert_eq!(spec.kind_ordinal, MANUAL_STRATEGY_KIND);
        assert_eq!(spec.folder_path, "Folder/Sub");
        assert_eq!(spec.fields, vec![("Depth".to_string(), "1.8".to_string())]);
        assert_eq!(spec.insert_after, None);
    }

    #[test]
    fn majority_fields_choose_only_strict_majorities() {
        let mut a = strategy(1, MANUAL_STRATEGY_KIND, "Manual C");
        a.fields = vec![
            ("BuyPrice".to_string(), "1.8".to_string()),
            ("OrderUsd".to_string(), "1000".to_string()),
        ];
        let mut b = strategy(2, MANUAL_STRATEGY_KIND, "Manual C");
        b.fields = vec![
            ("BuyPrice".to_string(), "1.8".to_string()),
            ("OrderUsd".to_string(), "500".to_string()),
        ];
        let mut c = strategy(3, MANUAL_STRATEGY_KIND, "Manual C");
        c.fields = vec![
            ("BuyPrice".to_string(), "2.4".to_string()),
            ("OrderUsd".to_string(), "500".to_string()),
        ];
        let rows = [
            (1, "A".to_string(), &a),
            (2, "B".to_string(), &b),
            (3, "C".to_string(), &c),
        ];

        let majorities = strict_majority_fields(1, &rows);

        assert_eq!(majorities["BuyPrice"].value, "1.8");
        assert_eq!(majorities["BuyPrice"].label, "Глубина выставления");
        assert_eq!(majorities["OrderUsd"].value, "500");
        assert_eq!(majorities["OrderUsd"].count, 2);
        assert_eq!(majorities["OrderUsd"].total, 3);
    }

    #[test]
    fn majority_fields_skip_ties() {
        let mut a = strategy(1, MANUAL_STRATEGY_KIND, "Manual D");
        a.fields = vec![("BuyPrice".to_string(), "1.8".to_string())];
        let mut b = strategy(2, MANUAL_STRATEGY_KIND, "Manual D");
        b.fields = vec![("BuyPrice".to_string(), "2.4".to_string())];
        let rows = [(1, "A".to_string(), &a), (2, "B".to_string(), &b)];

        assert!(strict_majority_fields(1, &rows).is_empty());
    }
}
