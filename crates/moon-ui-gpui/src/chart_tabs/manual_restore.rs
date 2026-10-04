//! Global manual-hook restore UI for a chart-tab window.

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use gpui::*;
use moon_core::feed::StrategyRow;
use moon_core::session::CoreId;
use moon_core::session::core_order::OrderedCores;
use moon_core::session::order_lines::ManualHookPlacement;
use moon_core::venue::CoreVenue;
use moon_ui::{
    MoonButton, MoonButtonVariant, MoonCheckbox, MoonDropdown, MoonMenuItem, MoonPalette,
    MoonScrollableElement, MoonSize, rgba_from,
};
use rust_i18n::t;

use super::ChartTabs;
use crate::Backend;
use crate::backend::MANUAL_STRATEGY_KIND;
use crate::controls::{
    CoreAllRowMode, core_combo, toggle_core_selection, toggle_exchange_cores, venue_section_label,
};

const MANUAL_HOOK_RESTORE_STEP_DELAY: Duration = Duration::from_millis(100);
const MOON_HOOK_STRATEGY_KIND: u8 = 20;

fn format_price_compact(price: f64) -> String {
    if price.abs() >= 1.0 {
        format!("{price:.4}")
    } else {
        format!("{price:.8}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn format_size_compact(size: f64) -> String {
    if size.abs() >= 1_000_000.0 {
        format!("{:.2}M", size / 1_000_000.0)
    } else if size.abs() >= 1_000.0 {
        format!("{:.2}K", size / 1_000.0)
    } else {
        format!("{size:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn restore_price_from_depth(placement: &ManualHookPlacement, current_price: Option<f64>) -> f64 {
    let depth = placement.depth_pct;
    let Some(current_price) = current_price.filter(|price| price.is_finite() && *price > 0.0)
    else {
        return placement.price;
    };
    if !(depth.is_finite() && depth >= 0.0 && depth < 100.0) {
        return placement.price;
    }
    let price = if placement.is_short {
        current_price * (1.0 + depth / 100.0)
    } else {
        current_price * (1.0 - depth / 100.0)
    };
    if price.is_finite() && price > 0.0 {
        price
    } else {
        placement.price
    }
}

fn normalized_strategy_text(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn looks_like_manual_hook_name(name: &str) -> bool {
    let name = name.trim().to_ascii_lowercase();
    name.starts_with("hook_") || name.starts_with("hook ")
}

fn manual_strategy_links_moon_hook(strategies: &[StrategyRow], manual: &StrategyRow) -> bool {
    let hook_names: Vec<String> = strategies
        .iter()
        .filter(|strategy| strategy.kind_ordinal == MOON_HOOK_STRATEGY_KIND)
        .map(|strategy| normalized_strategy_text(&strategy.name))
        .filter(|name| !name.is_empty())
        .collect();
    if hook_names.is_empty() {
        return false;
    }
    manual.fields.iter().any(|(name, value)| {
        let field_name = normalized_strategy_text(name);
        if !(field_name.contains("strategy") || field_name.contains("hook")) {
            return false;
        }
        let field_value = normalized_strategy_text(value);
        hook_names.iter().any(|hook| field_value == *hook)
    })
}

#[derive(Clone, PartialEq, Eq, Hash)]
pub(super) struct ManualHookRestoreKey {
    core: CoreId,
    market: String,
    is_short: bool,
    strategy_id: u64,
    source_uid: u64,
}

#[derive(Clone)]
struct ManualHookRestoreRow {
    key: ManualHookRestoreKey,
    core: CoreId,
    core_name: String,
    exchange_label: String,
    placement: ManualHookPlacement,
    resolved_strategy_id: Option<u64>,
    strategy_label: String,
    allowed: bool,
}

impl ChartTabs {
    pub(super) fn manual_hook_restore_count(&self, cx: &App) -> usize {
        self.manual_hook_restore_rows_for(self.backend.read(cx))
            .len()
    }

    pub(super) fn manual_hook_restore_is_open(&self) -> bool {
        self.manual_hook_restore_open
    }

    pub(super) fn render_manual_hook_restore(
        &mut self,
        palette: MoonPalette,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let all_rows = {
            let backend = self.backend.read(cx);
            self.manual_hook_restore_rows_for(&backend)
        };
        if all_rows.is_empty() {
            self.manual_hook_restore_selected.clear();
            if !self.manual_hook_restore_open {
                return None;
            }
        }
        let (core_options, venues) = {
            let backend = self.backend.read(cx);
            (
                backend.group_cores(self.group.as_str()),
                backend.session.core_venues(),
            )
        };
        let strategy_options = self.manual_hook_restore_strategy_options(&all_rows);
        let rows = self.filter_manual_hook_restore_rows(all_rows.clone());

        let entity = cx.entity();
        let selectable_count = rows
            .iter()
            .filter(|row| row.allowed && row.resolved_strategy_id.is_some())
            .count();
        let selected_count = rows
            .iter()
            .filter(|row| self.manual_hook_restore_selected.contains(&row.key))
            .count();
        let unselected_count = rows.len().saturating_sub(selected_count);
        let all_selected = selectable_count > 0 && selected_count == selectable_count;
        let open = self.manual_hook_restore_open;

        let panel = open.then(|| {
            let core_filter =
                self.render_manual_hook_core_filter(&core_options, &venues, entity.clone(), cx);
            let strategy_filter =
                self.render_manual_hook_strategy_filter(&strategy_options, entity.clone(), cx);
            let items = rows.iter().map(|row| {
                let key = row.key.clone();
                let checked = self.manual_hook_restore_selected.contains(&key);
                let active = row.allowed && row.resolved_strategy_id.is_some();
                let side = if row.placement.is_short {
                    t!("chart.manual_hooks.short").to_string()
                } else {
                    t!("chart.manual_hooks.long").to_string()
                };
                let strategy =
                    if row.resolved_strategy_id.is_some() && !row.strategy_label.is_empty() {
                        row.strategy_label.clone()
                    } else {
                        t!("chart.manual_hooks.strategy_missing").to_string()
                    };
                let row_text = t!(
                    "chart.manual_hooks.row",
                    core = row.core_name.clone(),
                    exchange = row.exchange_label.clone(),
                    market = row.placement.market.clone(),
                    side = side,
                    price = format_price_compact(row.placement.price),
                    size = format_size_compact(row.placement.quote_size)
                )
                .to_string();
                let toggle_entity = entity.clone();
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .px_2()
                    .py_1()
                    .rounded_sm()
                    .bg(if checked {
                        rgba_from(palette.blue, 0.11)
                    } else {
                        rgba_from(palette.chart_bg, 0.0)
                    })
                    .child(
                        MoonCheckbox::new(SharedString::from(format!(
                            "tabs-manual-hook-restore-{}-{}-{}",
                            row.core, row.placement.source_uid, row.placement.strategy_id
                        )))
                        .checked(checked)
                        .disabled(!active)
                        .on_change(
                            move |checked: &bool, _window, app| {
                                let checked = *checked;
                                let key = key.clone();
                                toggle_entity.update(app, |this, cx| {
                                    this.set_manual_hook_restore_selected(key, checked, cx);
                                });
                            },
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_xs()
                            .text_color(if active {
                                rgb(palette.text)
                            } else {
                                rgb(palette.text_muted)
                            })
                            .truncate()
                            .child(row_text),
                    )
                    .child(
                        div()
                            .max_w(px(148.0))
                            .px_2()
                            .py(px(1.0))
                            .rounded_sm()
                            .bg(rgba_from(palette.panel, 0.7))
                            .text_xs()
                            .text_color(if active {
                                rgb(palette.accent)
                            } else {
                                rgb(palette.text_muted)
                            })
                            .truncate()
                            .child(strategy),
                    )
            });

            let restore_disabled = selected_count == 0;
            let restore_label = if restore_disabled {
                t!("chart.manual_hooks.none_selected").to_string()
            } else {
                t!("chart.manual_hooks.restore_selected", n = selected_count).to_string()
            };
            let restore_entity = entity.clone();
            let close_entity = entity.clone();
            let select_all_entity = entity.clone();
            let clear_unselected_entity = entity.clone();
            let mut list = div().flex().flex_col().gap_1();
            if rows.is_empty() {
                list = list.child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_xs()
                        .text_color(rgb(palette.text_muted))
                        .child(if all_rows.is_empty() {
                            t!("chart.manual_hooks.no_rows").to_string()
                        } else {
                            t!("chart.manual_hooks.no_rows_for_filter").to_string()
                        }),
                );
            } else {
                list = list.children(items);
            }
            let list = div().flex_1().min_h(px(0.0)).w_full().child(
                div()
                    .id("tabs-manual-hook-restore-list")
                    .size_full()
                    .pr_1()
                    .child(list)
                    .overflow_y_scrollbar(),
            );
            div()
                .absolute()
                .top(px(42.0))
                .right(px(96.0))
                .w(px(620.0))
                .h(px(552.0))
                .p_3()
                .rounded_lg()
                .border_1()
                .border_color(rgb(palette.border))
                .bg(rgba_from(palette.chart_bg, 0.97))
                .shadow_lg()
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, |_, _window, app| {
                    app.stop_propagation();
                })
                .on_mouse_down(MouseButton::Right, |_, _window, app| {
                    app.stop_propagation();
                })
                .flex()
                .flex_col()
                .gap_2()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(FontWeight(700.0))
                                .text_color(rgb(palette.text))
                                .child(t!("chart.manual_hooks.title").to_string()),
                        )
                        .child(
                            MoonButton::new("tabs-manual-hook-restore-close")
                                .label("×")
                                .size(MoonSize::Xs)
                                .variant(MoonButtonVariant::Ghost)
                                .on_click(move |_, _window, app| {
                                    close_entity.update(app, |this, cx| {
                                        this.close_manual_hook_restore_panel(cx);
                                    });
                                })
                                .render(),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(rgb(palette.text_muted))
                        .child(t!("chart.manual_hooks.hint").to_string()),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(palette.text_muted))
                                .child(t!("chart.manual_hooks.core_filter").to_string()),
                        )
                        .child(core_filter),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            div()
                                .text_xs()
                                .text_color(rgb(palette.text_muted))
                                .child(t!("chart.manual_hooks.strategy_filter").to_string()),
                        )
                        .child(strategy_filter),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .px_2()
                        .py_1()
                        .rounded_sm()
                        .border_1()
                        .border_color(rgba_from(palette.border, 0.75))
                        .bg(rgba_from(palette.panel, 0.58))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(
                                    MoonCheckbox::new("tabs-manual-hook-restore-all")
                                        .checked(all_selected)
                                        .disabled(selectable_count == 0)
                                        .on_change(move |checked: &bool, _window, app| {
                                            let checked = *checked;
                                            select_all_entity.update(app, |this, cx| {
                                                this.set_all_manual_hook_restore_selected(
                                                    checked, cx,
                                                );
                                            });
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .font_weight(FontWeight(600.0))
                                        .text_color(rgb(palette.text))
                                        .child(t!("chart.manual_hooks.select_all").to_string()),
                                ),
                        )
                        .child(
                            div().text_xs().text_color(rgb(palette.text_muted)).child(
                                t!(
                                    "chart.manual_hooks.selected_summary",
                                    selected = selected_count,
                                    total = selectable_count
                                )
                                .to_string(),
                            ),
                        ),
                )
                .child(list)
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            MoonButton::new("tabs-manual-hook-restore-clear-unselected")
                                .label(
                                    t!("chart.manual_hooks.clear_unselected", n = unselected_count)
                                        .to_string(),
                                )
                                .size(MoonSize::Xs)
                                .variant(MoonButtonVariant::Danger)
                                .disabled(unselected_count == 0)
                                .on_click(move |_, _window, app| {
                                    clear_unselected_entity.update(app, |this, cx| {
                                        this.clear_unselected_manual_hooks(cx);
                                    });
                                })
                                .render(),
                        )
                        .child(
                            MoonButton::new("tabs-manual-hook-restore-apply")
                                .label(restore_label)
                                .size(MoonSize::Xs)
                                .variant(MoonButtonVariant::Blue)
                                .disabled(restore_disabled)
                                .on_click(move |_, _window, app| {
                                    restore_entity.update(app, |this, cx| {
                                        this.restore_selected_manual_hooks(cx);
                                    });
                                })
                                .render(),
                        ),
                )
        });

        panel.map(IntoElement::into_any_element)
    }

    fn manual_hook_restore_rows(&self, backend: &Backend) -> Vec<ManualHookRestoreRow> {
        let rows = self.manual_hook_restore_rows_for(backend);
        self.filter_manual_hook_restore_rows(rows)
    }

    fn manual_hook_restore_rows_for(&self, backend: &Backend) -> Vec<ManualHookRestoreRow> {
        let group = self.group.as_str();
        let venues = backend.session.core_venues();
        backend
            .session
            .sessions()
            .iter()
            .flat_map(|session| {
                let core = session.id;
                let allowed = backend.workspace_action_allows_core(Some(group), core);
                let core_name = session.name.clone();
                let venue = venues.get(&core);
                let exchange_label = venue_section_label(venue);
                backend
                    .session
                    .all_manual_hook_placements(core)
                    .into_iter()
                    .filter_map(move |placement| {
                        let resolved_strategy_id =
                            Self::resolve_manual_hook_strategy(backend, core, &placement);
                        if !Self::is_manual_hook_restore_strategy(
                            backend,
                            core,
                            &placement,
                            resolved_strategy_id,
                        ) {
                            return None;
                        }
                        let strategy_label = Self::manual_hook_restore_strategy_name(
                            backend,
                            core,
                            &placement,
                            resolved_strategy_id,
                        );
                        Some(ManualHookRestoreRow {
                            key: ManualHookRestoreKey {
                                core,
                                market: placement.market.clone(),
                                is_short: placement.is_short,
                                strategy_id: placement.strategy_id,
                                source_uid: placement.source_uid,
                            },
                            core,
                            core_name: core_name.clone(),
                            exchange_label: exchange_label.clone(),
                            placement,
                            resolved_strategy_id,
                            strategy_label,
                            allowed,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    fn filter_manual_hook_restore_rows(
        &self,
        rows: Vec<ManualHookRestoreRow>,
    ) -> Vec<ManualHookRestoreRow> {
        let rows = if self.manual_hook_restore_cores.is_empty() {
            rows
        } else {
            rows.into_iter()
                .filter(|row| self.manual_hook_restore_cores.contains(&row.core))
                .collect()
        };
        self.filter_manual_hook_restore_rows_by_strategy(rows)
    }

    fn filter_manual_hook_restore_rows_by_strategy(
        &self,
        rows: Vec<ManualHookRestoreRow>,
    ) -> Vec<ManualHookRestoreRow> {
        let Some(strategy) = self.manual_hook_restore_strategy.as_deref() else {
            return rows;
        };
        rows.into_iter()
            .filter(|row| row.strategy_label == strategy)
            .collect()
    }

    fn manual_hook_restore_strategy_options(&self, rows: &[ManualHookRestoreRow]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for row in rows {
            if row.strategy_label.is_empty()
                || out.iter().any(|name| name == row.strategy_label.as_str())
            {
                continue;
            }
            out.push(row.strategy_label.clone());
        }
        out
    }

    fn manual_hook_restore_strategy_label(&self, options: &[String]) -> String {
        match self.manual_hook_restore_strategy.as_deref() {
            None => t!("chart.manual_hooks.all_strategies").to_string(),
            Some(selected) => options
                .iter()
                .find(|name| name.as_str() == selected)
                .cloned()
                .unwrap_or_else(|| selected.to_string()),
        }
    }

    fn render_manual_hook_core_filter(
        &self,
        cores: &OrderedCores,
        venues: &HashMap<CoreId, CoreVenue>,
        entity: Entity<Self>,
        cx: &App,
    ) -> AnyElement {
        let toggle_entity = entity.clone();
        let section_entity = toggle_entity.clone();
        core_combo(
            "tabs-manual-hook-restore-core",
            cores,
            venues,
            &self.manual_hook_restore_cores,
            CoreAllRowMode::ImplicitOrComplete,
            t!("chart.manual_hooks.all_cores").to_string(),
            |n| t!("chart.manual_hooks.cores_n", n = n).to_string(),
            170.0,
            None,
            cx,
            move |core, app| {
                toggle_entity.update(app, |this, cx| {
                    this.toggle_manual_hook_restore_core(core, cx);
                });
            },
            move |cores, app| {
                section_entity.update(app, |this, cx| {
                    this.toggle_manual_hook_restore_section_cores(cores, cx);
                });
            },
        )
        .trigger_size(MoonSize::Xs.into())
        .into_any_element()
    }

    fn render_manual_hook_strategy_filter(
        &self,
        options: &[String],
        entity: Entity<Self>,
        _cx: &App,
    ) -> AnyElement {
        let selected = self.manual_hook_restore_strategy.clone();
        let mut items = Vec::with_capacity(options.len() + 1);
        items.push(
            MoonMenuItem::with_key(
                "mrestore-strategy-all",
                t!("chart.manual_hooks.all_strategies").to_string(),
            )
            .selected(selected.is_none())
            .on_click({
                let entity = entity.clone();
                move |_, _, app| {
                    entity.update(app, |this, cx| {
                        this.set_manual_hook_restore_strategy(None, cx);
                    });
                }
            }),
        );
        for strategy in options {
            let strategy_name = strategy.clone();
            let key = format!("mrestore-strategy-{strategy_name}");
            let entity = entity.clone();
            items.push(
                MoonMenuItem::with_key(key, strategy.clone())
                    .selected(selected.as_deref() == Some(strategy.as_str()))
                    .on_click(move |_, _, app| {
                        let strategy_name = strategy_name.clone();
                        entity.update(app, |this, cx| {
                            this.set_manual_hook_restore_strategy(Some(strategy_name), cx);
                        });
                    }),
            );
        }

        MoonDropdown::new("tabs-manual-hook-restore-strategy")
            .label(self.manual_hook_restore_strategy_label(options))
            .trigger_caret(true)
            .trigger_variant(MoonButtonVariant::Soft)
            .trigger_size(MoonSize::Xs.into())
            .fit_trigger_width(112.0, 180.0)
            .fit_menu_width(160.0, 300.0)
            .menu_size(MoonSize::Xs)
            .menu_max_height_ui(220.0)
            .items(items)
            .into_any_element()
    }

    fn resolve_manual_hook_strategy(
        backend: &Backend,
        core: CoreId,
        placement: &ManualHookPlacement,
    ) -> Option<u64> {
        let core_data = backend.session.store().core(core)?;
        if core_data
            .strategies
            .iter()
            .any(|strategy| strategy.id == placement.strategy_id)
        {
            return Some(placement.strategy_id);
        }
        if placement.strategy_name.is_empty() {
            return None;
        }
        core_data
            .strategies
            .iter()
            .find(|strategy| strategy.name == placement.strategy_name)
            .map(|strategy| strategy.id)
    }

    fn is_manual_hook_restore_strategy(
        backend: &Backend,
        core: CoreId,
        placement: &ManualHookPlacement,
        resolved_strategy_id: Option<u64>,
    ) -> bool {
        let Some(core_data) = backend.session.store().core(core) else {
            return looks_like_manual_hook_name(&placement.strategy_name);
        };
        let Some(strategy_id) = resolved_strategy_id else {
            return looks_like_manual_hook_name(&placement.strategy_name);
        };
        let Some(strategy) = core_data
            .strategies
            .iter()
            .find(|strategy| strategy.id == strategy_id)
        else {
            return looks_like_manual_hook_name(&placement.strategy_name);
        };
        if strategy.kind_ordinal != MANUAL_STRATEGY_KIND {
            return false;
        }
        manual_strategy_links_moon_hook(&core_data.strategies, strategy)
            || looks_like_manual_hook_name(&strategy.name)
    }

    fn manual_hook_restore_strategy_name(
        backend: &Backend,
        core: CoreId,
        placement: &ManualHookPlacement,
        resolved_strategy_id: Option<u64>,
    ) -> String {
        resolved_strategy_id
            .and_then(|strategy_id| {
                backend.session.store().core(core).and_then(|core_data| {
                    core_data
                        .strategies
                        .iter()
                        .find(|strategy| strategy.id == strategy_id)
                        .map(|strategy| strategy.name.clone())
                })
            })
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| placement.strategy_name.clone())
    }

    pub(super) fn open_manual_hook_restore_panel(&mut self, cx: &mut Context<Self>) {
        self.manual_hook_restore_open = true;
        let rows = {
            let backend = self.backend.read(cx);
            self.manual_hook_restore_rows(&backend)
        };
        let valid: HashSet<_> = rows.iter().map(|row| row.key.clone()).collect();
        self.manual_hook_restore_selected
            .retain(|key| valid.contains(key));
        if self.manual_hook_restore_selected.is_empty() {
            self.manual_hook_restore_selected.extend(
                rows.iter()
                    .filter(|row| row.allowed && row.resolved_strategy_id.is_some())
                    .map(|row| row.key.clone()),
            );
        }
        cx.notify();
    }

    pub(super) fn close_manual_hook_restore_panel(&mut self, cx: &mut Context<Self>) {
        self.manual_hook_restore_open = false;
        cx.notify();
    }

    fn set_manual_hook_restore_selected(
        &mut self,
        key: ManualHookRestoreKey,
        selected: bool,
        cx: &mut Context<Self>,
    ) {
        if selected {
            self.manual_hook_restore_selected.insert(key);
        } else {
            self.manual_hook_restore_selected.remove(&key);
        }
        cx.notify();
    }

    fn set_all_manual_hook_restore_selected(&mut self, selected: bool, cx: &mut Context<Self>) {
        let rows = {
            let backend = self.backend.read(cx);
            self.manual_hook_restore_rows(&backend)
        };
        let valid: HashSet<_> = rows.iter().map(|row| row.key.clone()).collect();
        self.manual_hook_restore_selected
            .retain(|key| valid.contains(key));
        if selected {
            self.manual_hook_restore_selected.extend(
                rows.into_iter()
                    .filter(|row| row.allowed && row.resolved_strategy_id.is_some())
                    .map(|row| row.key),
            );
        } else {
            for row in rows {
                self.manual_hook_restore_selected.remove(&row.key);
            }
        }
        cx.notify();
    }

    fn toggle_manual_hook_restore_core(&mut self, core: Option<CoreId>, cx: &mut Context<Self>) {
        if toggle_core_selection(&mut self.manual_hook_restore_cores, core) {
            cx.notify();
        }
    }

    fn toggle_manual_hook_restore_section_cores(
        &mut self,
        cores: Vec<CoreId>,
        cx: &mut Context<Self>,
    ) {
        let available: HashSet<CoreId> = {
            let backend = self.backend.read(cx);
            backend
                .group_cores(self.group.as_str())
                .iter()
                .map(|(core, _)| *core)
                .collect()
        };
        if toggle_exchange_cores(&mut self.manual_hook_restore_cores, &available, cores) {
            cx.notify();
        }
    }

    fn set_manual_hook_restore_strategy(
        &mut self,
        strategy: Option<String>,
        cx: &mut Context<Self>,
    ) {
        if self.manual_hook_restore_strategy != strategy {
            self.manual_hook_restore_strategy = strategy;
            cx.notify();
        }
    }

    fn clear_unselected_manual_hooks(&mut self, cx: &mut Context<Self>) {
        let rows = {
            let backend = self.backend.read(cx);
            self.manual_hook_restore_rows(&backend)
        };
        let selected = self.manual_hook_restore_selected.clone();
        let mut forgotten: Vec<(CoreId, String, ManualHookPlacement)> = Vec::new();
        for row in rows {
            if selected.contains(&row.key) {
                continue;
            }
            forgotten.push((row.core, row.placement.market.clone(), row.placement));
        }
        if forgotten.is_empty() {
            return;
        }
        self.backend.update(cx, |backend, _| {
            for (core, market, placement) in &forgotten {
                backend.session.forget_manual_hook_placements(
                    *core,
                    market,
                    std::slice::from_ref(placement),
                );
            }
        });
        let valid: HashSet<_> = {
            let backend = self.backend.read(cx);
            self.manual_hook_restore_rows(&backend)
                .into_iter()
                .map(|row| row.key)
                .collect()
        };
        self.manual_hook_restore_selected
            .retain(|key| valid.contains(key));
        if valid.is_empty() {
            self.manual_hook_restore_open = false;
        }
        cx.notify();
    }

    fn restore_manual_hook_row(
        &mut self,
        row: ManualHookRestoreRow,
        workspace_group: &str,
        cx: &mut Context<Self>,
    ) {
        self.backend.update(cx, |backend, _| {
            if !backend.workspace_action_allows_core(Some(workspace_group), row.core) {
                return;
            }
            let Some(strategy_id) =
                Self::resolve_manual_hook_strategy(backend, row.core, &row.placement)
            else {
                log::warn!(
                    "manual hook restore skipped: strategy missing core={} market={} strategy={} uid={}",
                    moon_core::feed::core_label(row.core),
                    row.placement.market,
                    row.placement.strategy_name,
                    row.placement.source_uid
                );
                return;
            };
            let restore_price = restore_price_from_depth(
                &row.placement,
                backend
                    .session
                    .market_source()
                    .latest_price(row.core, &row.placement.market)
                    .ok()
                    .map(f64::from),
            );
            let restore_size_usd = row.placement.quote_size;
            let Some(restore_size_base) = backend.manual_order_size_base_from_usd(
                row.core,
                &row.placement.market,
                restore_price,
                restore_size_usd,
            ) else {
                log::warn!(
                    "manual hook restore skipped: size conversion unavailable core={} market={} usd={} uid={}",
                    moon_core::feed::core_label(row.core),
                    row.placement.market,
                    restore_size_usd,
                    row.placement.source_uid
                );
                return;
            };
            let previous_manual = backend.manual_strat_state(row.core);
            backend.set_manual_strategy(row.core, true, strategy_id);
            let Some(terms) = backend.manual_order_terms(
                row.core,
                &row.placement.market,
                restore_price,
                row.placement.is_short,
                Some(restore_size_base),
            ) else {
                backend.set_manual_strategy(row.core, previous_manual.0, previous_manual.1);
                log::warn!(
                    "manual hook restore skipped: manual terms unavailable core={} market={} uid={}",
                    moon_core::feed::core_label(row.core),
                    row.placement.market,
                    row.placement.source_uid
                );
                return;
            };
            match backend.session.place_order(
                row.core,
                row.placement.market.clone(),
                row.placement.is_short,
                restore_price,
                terms.size_base,
                terms.strategy_id.or(Some(strategy_id)),
                terms.exit,
                terms.planned_sell.unwrap_or(0.0),
                terms.sync_exit,
            ) {
                Ok(()) => {
                    log::info!(
                        "manual hook restored: core={} market={} manual_strategy={} price={} saved_price={} depth_pct={} quote_size={}",
                        moon_core::feed::core_label(row.core),
                        row.placement.market,
                        strategy_id,
                        restore_price,
                        row.placement.price,
                        row.placement.depth_pct,
                        restore_size_usd
                    );
                    backend.session.forget_manual_hook_placements(
                        row.core,
                        &row.placement.market,
                        std::slice::from_ref(&row.placement),
                    );
                }
                Err(error) => {
                    log::warn!(
                        "manual hook restore failed: core={} market={} manual_strategy={} uid={}: {error:#}",
                        moon_core::feed::core_label(row.core),
                        row.placement.market,
                        strategy_id,
                        row.placement.source_uid
                    );
                }
            }
            backend.set_manual_strategy(row.core, previous_manual.0, previous_manual.1);
        });
        cx.notify();
    }

    fn restore_selected_manual_hooks(&mut self, cx: &mut Context<Self>) {
        let rows = {
            let backend = self.backend.read(cx);
            self.manual_hook_restore_rows(&backend)
        };
        let selected = self.manual_hook_restore_selected.clone();
        let workspace_group = self.group.clone();
        let restore_rows: Vec<_> = rows
            .into_iter()
            .filter(|row| selected.contains(&row.key))
            .collect();
        self.manual_hook_restore_selected.clear();
        self.manual_hook_restore_open = false;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let executor = cx.update(|cx| cx.background_executor().clone());
            for (index, row) in restore_rows.into_iter().enumerate() {
                if index > 0 {
                    executor.timer(MANUAL_HOOK_RESTORE_STEP_DELAY).await;
                }
                let alive = cx.update(|cx| {
                    this.update(cx, |this, cx| {
                        this.restore_manual_hook_row(row, workspace_group.as_str(), cx);
                    })
                    .is_ok()
                });
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }
}
