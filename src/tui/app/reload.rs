use std::collections::HashSet;

use crate::config::{BarConfig, ModuleParamValue, ModuleSectionView, ParamWriteExpect};

use super::text::clamped_scroll_offset;
use super::{
    App, Mode, ModuleParamsState, ModuleParamsStatus, PanelFocus, ParamEntry, ParamOrigin,
};

impl App {
    pub(super) fn sync_preset_pointer_from_disk(&mut self) -> bool {
        let Some(config_dir) = self.config_dir.as_deref() else {
            return false;
        };
        match (
            crate::config::active_config_path(config_dir),
            crate::config::read_active_name(config_dir),
        ) {
            (Ok(path), Ok(active)) => {
                let changed = self.config_path.as_deref() != Some(path.as_path())
                    || self.active_preset.as_deref() != Some(active.as_str());
                self.config_path = Some(path);
                self.active_preset = Some(active);
                changed
            }
            _ => {
                let had = self.config_path.is_some() || self.active_preset.is_some();
                self.config_path = None;
                self.active_preset = None;
                had
            }
        }
    }

    pub(super) fn ensure_preset_pointer_current(&mut self) {
        if self.sync_preset_pointer_from_disk() {
            self.refresh_config();
        }
    }

    pub(super) fn notify_daemon_config_reload(&mut self) {
        match crate::control::notify_running(crate::reload::ReloadRequest::config()) {
            Ok(crate::control::NotifyOutcome::Delivered) => {}
            Ok(crate::control::NotifyOutcome::NotRunning) => {
                self.push_action_message(
                    "smstatus is not running; config saved but bar not updated".to_string(),
                );
            }
            Err(err) => {
                self.push_action_message(format!("failed to notify running daemon: {err}"));
            }
        }
    }

    pub(super) fn refresh_config(&mut self) {
        self.sync_preset_pointer_from_disk();
        let Some(path) = self.config_path.as_deref() else {
            self.clear_loaded_config_state();
            self.refresh_installed_extensions();
            self.drop_stale_confirming_remove_mode();
            self.drop_stale_param_modes();
            self.drop_stale_preset_modes();
            return;
        };
        match BarConfig::load(path) {
            Ok(config) => {
                crate::logging::set_retain_days(config.log_days());
                self.separator = Some(config.separator());
                self.last_separator_error = None;
                match config.module_names() {
                    Ok(names) => {
                        self.module_scroll_offset = self
                            .module_scroll_offset
                            .min(names.len().saturating_sub(self.modules_viewport_height));
                        let previous_entry = self
                            .selected_index
                            .and_then(|i| self.modules.as_ref().and_then(|m| m.get(i).cloned()));
                        self.selected_index = if names.is_empty() {
                            None
                        } else {
                            Some(self.selected_index.unwrap_or(0).min(names.len() - 1))
                        };
                        self.modules = Some(names);
                        self.last_modules_error = None;
                        self.prune_metadata_to_configured();
                        self.prune_schema_to_configured();
                        self.ensure_selected_visible();
                        let new_entry = self
                            .selected_index
                            .and_then(|i| self.modules.as_ref().and_then(|m| m.get(i).cloned()));
                        let selection_changed = previous_entry != new_entry;
                        self.rebuild_module_params_from(&config, selection_changed);
                        if self.selected_index.is_none() {
                            self.panel_focus = PanelFocus::Modules;
                        }
                    }
                    Err(err) => {
                        self.modules = None;
                        self.clear_metadata_state();
                        self.clear_schema_state();
                        self.selected_index = None;
                        self.module_params = None;
                        self.panel_focus = PanelFocus::Modules;
                        let message = err.to_string();
                        if self.last_modules_error.as_deref() != Some(message.as_str()) {
                            self.push_action_message(format!("Failed to read modules: {message}"));
                            self.last_modules_error = Some(message);
                        }
                    }
                }
                self.config_cache = Some(config);
            }
            Err(err) => {
                self.separator = None;
                self.modules = None;
                self.clear_metadata_state();
                self.clear_schema_state();
                self.selected_index = None;
                self.module_params = None;
                self.config_cache = None;
                self.panel_focus = PanelFocus::Modules;
                let message = err.to_string();
                if self.last_separator_error.as_deref() != Some(message.as_str()) {
                    self.push_action_message(format!("Failed to read config: {message}"));
                    self.last_separator_error = Some(message);
                }
            }
        }
        self.refresh_installed_extensions();
        self.drop_stale_confirming_remove_mode();
        self.drop_stale_param_modes();
        self.drop_stale_preset_modes();
    }

    pub(super) fn drop_stale_preset_modes(&mut self) {
        let Some(config_dir) = self.config_dir.as_deref() else {
            if matches!(
                self.mode,
                Mode::ChoosingPreset { .. }
                    | Mode::NamingPreset { .. }
                    | Mode::ConfirmingRemovePreset { .. }
            ) {
                self.mode = Mode::Normal;
            }
            return;
        };
        match &self.mode {
            Mode::ChoosingPreset { names, .. } => {
                let Ok(current) = crate::config::list_preset_names(config_dir) else {
                    self.mode = Mode::Normal;
                    return;
                };
                let Ok(active) = crate::config::read_active_name(config_dir) else {
                    self.mode = Mode::Normal;
                    return;
                };
                if current != *names || self.active_preset.as_deref() != Some(active.as_str()) {
                    if current.is_empty() {
                        self.mode = Mode::Normal;
                    } else {
                        self.mode = Mode::ChoosingPreset {
                            names: current,
                            selected: 0,
                            scroll_offset: 0,
                        };
                    }
                }
            }
            Mode::ConfirmingRemovePreset { name } => {
                let Ok(current) = crate::config::list_preset_names(config_dir) else {
                    self.mode = Mode::Normal;
                    return;
                };
                if !current.iter().any(|n| n == name) {
                    self.mode = Mode::Normal;
                }
            }
            Mode::NamingPreset { .. } if crate::config::active_config_path(config_dir).is_err() => {
                self.mode = Mode::Normal;
            }
            _ => {}
        }
    }

    fn clear_loaded_config_state(&mut self) {
        self.separator = None;
        self.modules = None;
        self.clear_metadata_state();
        self.clear_schema_state();
        self.selected_index = None;
        self.module_params = None;
        self.config_cache = None;
        self.panel_focus = PanelFocus::Modules;
        self.last_separator_error = None;
        self.last_modules_error = None;
    }

    fn clear_metadata_state(&mut self) {
        self.metadata_by_kind.clear();
        self.metadata_failed.clear();
        self.metadata_needs_stable.clear();
        self.required_extensions_by_kind.clear();
    }

    fn clear_schema_state(&mut self) {
        self.schema_by_kind.clear();
        self.schema_failed.clear();
        self.schema_needs_stable.clear();
    }

    fn prune_metadata_to_configured(&mut self) {
        let configured = self.configured_kinds();
        self.metadata_by_kind
            .retain(|kind, _| configured.contains(kind));
        self.metadata_failed
            .retain(|kind| configured.contains(kind));
        self.metadata_needs_stable
            .retain(|kind| configured.contains(kind));
        self.required_extensions_by_kind
            .retain(|kind, _| configured.contains(kind));
    }

    fn prune_schema_to_configured(&mut self) {
        let configured = self.configured_kinds();
        self.schema_by_kind
            .retain(|kind, _| configured.contains(kind));
        self.schema_failed.retain(|kind| configured.contains(kind));
        self.schema_needs_stable
            .retain(|kind| configured.contains(kind));
    }

    pub(super) fn refresh_wasm_derived_state_for_kinds(&mut self, wasm_kinds: &[String]) {
        let configured = self.configured_kinds();
        for kind in wasm_kinds {
            if !configured.contains(kind) {
                continue;
            }
            self.metadata_by_kind.remove(kind);
            self.metadata_failed.remove(kind);
            self.metadata_needs_stable.insert(kind.clone());
            self.required_extensions_by_kind.remove(kind);
            self.schema_by_kind.remove(kind);
            self.schema_failed.remove(kind);
            self.schema_needs_stable.insert(kind.clone());
        }
        self.refresh_extension_display_cache();
    }

    pub(in crate::tui) fn poll_metadata(&mut self) {
        let configured = self.configured_kinds();
        let Some(kind) = configured.into_iter().find(|kind| {
            !self.metadata_by_kind.contains_key(kind) && !self.metadata_failed.contains(kind)
        }) else {
            return;
        };
        let Some(modules_dir) = self.modules_dir.clone() else {
            return;
        };
        let wait_stable = self.metadata_needs_stable.remove(&kind);
        if wait_stable {
            crate::module::wait_wasm_stable(&crate::manifest::module_manifest_path(
                &modules_dir,
                &kind,
            ));
        }
        match crate::manifest::read_module_manifest(&modules_dir, &kind) {
            Ok(manifest) => {
                self.required_extensions_by_kind
                    .insert(kind.clone(), manifest.required_extensions.clone());
                self.metadata_by_kind.insert(kind, manifest.to_metadata());
            }
            Err(_) => {
                self.required_extensions_by_kind.remove(&kind);
                self.metadata_failed.insert(kind);
            }
        }
        self.refresh_extension_display_cache();
    }

    fn configured_kinds(&self) -> HashSet<String> {
        let Some(modules) = &self.modules else {
            return HashSet::new();
        };
        modules
            .iter()
            .map(|entry| BarConfig::split_module_entry(entry).0.to_string())
            .collect()
    }

    fn selected_kind(&self) -> Option<String> {
        let idx = self.selected_index?;
        let entry = self.modules.as_ref()?.get(idx)?;
        Some(BarConfig::split_module_entry(entry).0.to_string())
    }

    pub(in crate::tui) fn poll_schema(&mut self) {
        let configured = self.configured_kinds();
        let Some(kind) = configured.into_iter().find(|kind| {
            !self.schema_by_kind.contains_key(kind) && !self.schema_failed.contains(kind)
        }) else {
            return;
        };
        let Some(modules_dir) = self.modules_dir.clone() else {
            return;
        };
        if self.schema_probe.is_none() {
            match crate::schema_probe::SchemaProbe::new() {
                Ok(probe) => self.schema_probe = Some(probe),
                Err(_) => return,
            }
        }
        let Some(probe) = self.schema_probe.as_ref() else {
            return;
        };
        let wait_stable = self.schema_needs_stable.remove(&kind);
        let result = if wait_stable {
            probe.read_after_stable(&modules_dir, &kind)
        } else {
            probe.read(&modules_dir, &kind)
        };
        match result {
            Ok(schema) => {
                self.schema_by_kind.insert(kind.clone(), schema);
                if self.selected_kind().as_deref() == Some(kind.as_str()) {
                    self.rebuild_module_params(false);
                }
            }
            Err(_) => {
                self.schema_failed.insert(kind);
            }
        }
    }

    pub(super) fn rebuild_module_params_from(&mut self, config: &BarConfig, reset_selection: bool) {
        let Some(idx) = self.selected_index else {
            self.module_params = None;
            return;
        };
        let Some(entry) = self.modules.as_ref().and_then(|m| m.get(idx)) else {
            self.module_params = None;
            return;
        };
        let kind = BarConfig::split_module_entry(entry).0.to_string();
        let section_name = BarConfig::split_module_entry(entry).1.to_string();
        let view = config.module_section_string_entries(&section_name);
        let (status, raw_entries) = match view {
            ModuleSectionView::Missing => (
                ModuleParamsStatus::Missing {
                    section: section_name,
                },
                Vec::new(),
            ),
            ModuleSectionView::Empty => (ModuleParamsStatus::Empty, Vec::new()),
            ModuleSectionView::Entries(raw) => (ModuleParamsStatus::Entries, raw),
        };
        let mut entries: Vec<ParamEntry> = raw_entries
            .into_iter()
            .map(|(key, value)| ParamEntry {
                key,
                value,
                origin: ParamOrigin::Explicit,
            })
            .collect();
        let explicit_keys: HashSet<String> = entries.iter().map(|e| e.key.clone()).collect();
        if let Some(schema) = self.schema_by_kind.get(&kind) {
            for param in schema {
                if explicit_keys.contains(param.name.as_str()) {
                    continue;
                }
                entries.push(ParamEntry {
                    key: param.name.clone(),
                    value: ModuleParamValue::String(param.default.clone()),
                    origin: ParamOrigin::Default,
                });
            }
        }
        let status = if entries.is_empty() {
            status
        } else {
            ModuleParamsStatus::Entries
        };
        let selected_index = if entries.is_empty() {
            None
        } else if reset_selection {
            Some(0)
        } else {
            let prev = self
                .module_params
                .as_ref()
                .and_then(|p| p.selected_index)
                .unwrap_or(0);
            Some(prev.min(entries.len() - 1))
        };
        let scroll_offset = if reset_selection {
            0
        } else {
            self.module_params
                .as_ref()
                .map(|p| p.scroll_offset)
                .unwrap_or(0)
        };
        let mut state = ModuleParamsState {
            status,
            entries,
            selected_index,
            scroll_offset,
        };
        if let Some(sel) = state.selected_index {
            state.scroll_offset = clamped_scroll_offset(
                state.scroll_offset,
                sel,
                self.params_viewport_height
                    .saturating_sub(self.detail_header_line_count()),
            );
        }
        self.module_params = Some(state);
    }

    pub(super) fn rebuild_module_params(&mut self, reset_selection: bool) {
        let Some(config) = self.config_cache.take() else {
            self.module_params = None;
            return;
        };
        self.rebuild_module_params_from(&config, reset_selection);
        self.config_cache = Some(config);
    }

    pub(super) fn drop_stale_confirming_remove_mode(&mut self) {
        let Mode::ConfirmingRemove { index, name } = &self.mode else {
            return;
        };
        let still_armed = self
            .modules
            .as_ref()
            .and_then(|modules| modules.get(*index))
            .is_some_and(|current| current == name);
        if !still_armed {
            self.mode = Mode::Normal;
        }
    }

    pub(super) fn drop_stale_param_modes(&mut self) {
        let current_section = self.selected_section_name();
        let key_still_present = |key: &str| {
            self.module_params
                .as_ref()
                .is_some_and(|p| p.entries.iter().any(|e| e.key == key))
        };
        let should_drop = match &self.mode {
            Mode::AddingParamKey { section, .. } => {
                current_section.as_deref() != Some(section.as_str())
            }
            Mode::EditingParamValue {
                section,
                key,
                expect,
                ..
            } => {
                if current_section.as_deref() != Some(section.as_str()) {
                    true
                } else if matches!(expect, ParamWriteExpect::KeyAbsent) {
                    key_still_present(key)
                } else {
                    !key_still_present(key)
                }
            }
            Mode::ConfirmingRemoveParam { section, key }
            | Mode::RenamingParamKey {
                section,
                old_key: key,
                ..
            } => current_section.as_deref() != Some(section.as_str()) || !key_still_present(key),
            _ => false,
        };
        if should_drop {
            self.mode = Mode::Normal;
        }
    }
}
