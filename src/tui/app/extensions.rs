use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::{apply_text_edit, clamped_scroll_offset};
use super::{App, DetailContext, InstallTarget, Mode, PanelFocus};

impl App {
    pub(super) fn refresh_installed_extensions(&mut self) {
        let Some(extensions_dir) = self.extensions_dir.as_ref() else {
            self.installed_extensions.clear();
            self.extension_selected_index = None;
            self.refresh_extension_display_cache();
            return;
        };
        match crate::install::list_extensions_in(extensions_dir) {
            Ok(names) => self.installed_extensions = names,
            Err(err) => {
                self.push_action_message(format!("Failed to list extensions: {err}"));
                self.installed_extensions.clear();
            }
        }
        if self.installed_extensions.is_empty() {
            self.extension_selected_index = None;
            self.extension_scroll_offset = 0;
        } else {
            let max = self.installed_extensions.len() - 1;
            self.extension_selected_index =
                Some(self.extension_selected_index.unwrap_or(0).min(max));
            self.ensure_extension_selected_visible();
        }
        self.refresh_extension_display_cache();
    }

    pub(super) fn focus_extensions(&mut self) {
        self.refresh_installed_extensions();
        self.panel_focus = PanelFocus::Extensions;
    }

    pub(super) fn begin_install(&mut self) {
        self.mode = Mode::ChoosingInstallKind { selected: 0 };
    }

    pub(super) fn ensure_extension_selected_visible(&mut self) {
        let Some(idx) = self.extension_selected_index else {
            return;
        };
        self.extension_scroll_offset = clamped_scroll_offset(
            self.extension_scroll_offset,
            idx,
            self.extensions_viewport_height,
        );
    }

    pub(super) fn select_previous_extension(&mut self) {
        let Some(idx) = self.extension_selected_index else {
            return;
        };
        if idx > 0 {
            self.extension_selected_index = Some(idx - 1);
            self.ensure_extension_selected_visible();
        }
    }

    pub(super) fn select_next_extension(&mut self) {
        let Some(idx) = self.extension_selected_index else {
            return;
        };
        if idx + 1 < self.installed_extensions.len() {
            self.extension_selected_index = Some(idx + 1);
            self.ensure_extension_selected_visible();
        }
    }

    pub(super) fn handle_key_normal_extensions(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => self.select_previous_extension(),
            KeyCode::Down => self.select_next_extension(),
            KeyCode::Char('i') => self.begin_install(),
            KeyCode::Enter | KeyCode::Right => self.focus_params(),
            KeyCode::Esc | KeyCode::Left => self.panel_focus = PanelFocus::Modules,
            KeyCode::Tab => {
                if self.selected_index.is_some() {
                    self.detail_context = DetailContext::Module;
                    self.panel_focus = PanelFocus::Params;
                } else {
                    self.focus_logs();
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_key_choosing_install_kind(&mut self, key: KeyEvent) {
        let Mode::ChoosingInstallKind { selected } = &mut self.mode else {
            return;
        };
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Up => *selected = selected.saturating_sub(1),
            KeyCode::Down => *selected = (*selected + 1).min(1),
            KeyCode::Enter => {
                let target = if *selected == 0 {
                    InstallTarget::Module
                } else {
                    InstallTarget::Extension
                };
                self.mode = Mode::EnteringInstallSource {
                    target,
                    buffer: String::new(),
                    cursor: 0,
                };
            }
            _ => {}
        }
    }

    pub(super) fn handle_key_entering_install_source(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => self.commit_install_source(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::EnteringInstallSource { buffer, cursor, .. } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    pub(super) fn handle_key_entering_install_sha256(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => self.mode = Mode::Normal,
            KeyCode::Enter => self.commit_install_sha256(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::EnteringInstallSha256 { buffer, cursor, .. } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    fn commit_install_source(&mut self) {
        let Mode::EnteringInstallSource { target, buffer, .. } = &self.mode else {
            return;
        };
        if buffer.trim().is_empty() {
            self.push_action_message("Install source cannot be empty".to_string());
            return;
        }
        match target {
            InstallTarget::Module if self.modules_dir.is_none() => {
                self.push_action_message(
                    "cannot install module: modules directory unknown".to_string(),
                );
                return;
            }
            InstallTarget::Extension if self.extensions_dir.is_none() => {
                self.push_action_message(
                    "cannot install extension: extensions directory unknown".to_string(),
                );
                return;
            }
            _ => {}
        }
        let Mode::EnteringInstallSource { target, buffer, .. } = std::mem::take(&mut self.mode)
        else {
            return;
        };
        let source = buffer.trim().to_string();
        if crate::install::is_remote_install_source(&source) {
            self.mode = Mode::EnteringInstallSha256 {
                target,
                source,
                buffer: String::new(),
                cursor: 0,
            };
            return;
        }
        self.run_install(target, source, crate::install::InstallOptions::default());
    }

    fn commit_install_sha256(&mut self) {
        let Mode::EnteringInstallSha256 {
            target: _,
            source: _,
            buffer,
            ..
        } = &self.mode
        else {
            return;
        };
        if buffer.trim().is_empty() {
            self.push_action_message("remote install requires a SHA-256 hash".to_string());
            return;
        }
        let Mode::EnteringInstallSha256 {
            target,
            source,
            buffer,
            ..
        } = std::mem::take(&mut self.mode)
        else {
            return;
        };
        let options = crate::install::InstallOptions {
            expected_sha256: Some(buffer.trim().to_string()),
            ..Default::default()
        };
        self.run_install(target, source, options);
    }

    fn run_install(
        &mut self,
        target: InstallTarget,
        source: String,
        options: crate::install::InstallOptions,
    ) {
        self.mode = Mode::Normal;
        match target {
            InstallTarget::Module => {
                let modules_dir = self.modules_dir.clone().expect("checked above");
                match crate::install::install_module_into(&modules_dir, &source, &options) {
                    Ok(output) => {
                        for warning in &output.warnings {
                            self.push_action_message(warning.clone());
                        }
                        let kind = match &output.value {
                            crate::install::ModuleInstallOutcome::Fresh { kind, .. }
                            | crate::install::ModuleInstallOutcome::Skip { kind, .. }
                            | crate::install::ModuleInstallOutcome::Replace { kind, .. } => {
                                kind.clone()
                            }
                        };
                        self.push_action_message(crate::install::format_module_outcome(
                            &output.value,
                        ));
                        self.refresh_wasm_derived_state_for_kinds(&[kind]);
                    }
                    Err(err) => self.push_action_message(err.to_string()),
                }
            }
            InstallTarget::Extension => {
                let extensions_dir = self.extensions_dir.clone().expect("checked above");
                match crate::install::install_extension_into(&extensions_dir, &source, &options) {
                    Ok(output) => {
                        for warning in &output.warnings {
                            self.push_action_message(warning.clone());
                        }
                        self.push_action_message(crate::install::format_extension_outcome(
                            &output.value,
                        ));
                        self.refresh_installed_extensions();
                    }
                    Err(err) => self.push_action_message(err.to_string()),
                }
            }
        }
    }

    pub(in crate::tui) fn extension_list_labels(&self) -> &[String] {
        &self.extension_overlay_labels
    }

    pub(in crate::tui) fn install_kind_labels() -> [&'static str; 2] {
        ["module", "extension"]
    }
}
