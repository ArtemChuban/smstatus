use ratatui::crossterm::event::{KeyCode, KeyEvent};

use super::text::{apply_text_edit, clamped_scroll_offset};
use super::{App, InstallTarget, Mode};

impl App {
    pub(super) fn refresh_installed_extensions(&mut self) {
        let Some(extensions_dir) = self.extensions_dir.as_ref() else {
            self.installed_extensions.clear();
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
        self.refresh_extension_display_cache();
    }

    pub(super) fn begin_browse_extensions(&mut self) {
        self.refresh_installed_extensions();
        self.mode = Mode::BrowsingExtensions {
            selected: 0,
            scroll_offset: 0,
        };
    }

    pub(super) fn begin_install(&mut self) {
        self.mode = Mode::ChoosingInstallKind { selected: 0 };
    }

    pub(super) fn handle_key_browsing_extensions(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                return;
            }
            KeyCode::Char('i') => {
                self.begin_install();
                return;
            }
            _ => {}
        }
        let Mode::BrowsingExtensions {
            selected,
            scroll_offset,
        } = &mut self.mode
        else {
            return;
        };
        match key.code {
            KeyCode::Up => {
                *selected = selected.saturating_sub(1);
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.overlay_viewport_height);
            }
            KeyCode::Down => {
                if !self.installed_extensions.is_empty() {
                    *selected = (*selected + 1).min(self.installed_extensions.len() - 1);
                }
                *scroll_offset =
                    clamped_scroll_offset(*scroll_offset, *selected, self.overlay_viewport_height);
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
            KeyCode::Enter => self.commit_install(),
            KeyCode::Left | KeyCode::Right | KeyCode::Backspace | KeyCode::Char(_) => {
                if let Mode::EnteringInstallSource { buffer, cursor, .. } = &mut self.mode {
                    apply_text_edit(buffer, cursor, key);
                }
            }
            _ => {}
        }
    }

    pub(super) fn commit_install(&mut self) {
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
        let source = buffer.trim();
        match target {
            InstallTarget::Module => {
                let modules_dir = self.modules_dir.clone().expect("checked above");
                match crate::install::install_module_into(&modules_dir, source) {
                    Ok(outcome) => {
                        let kind = match &outcome {
                            crate::install::ModuleInstallOutcome::Fresh { kind, .. }
                            | crate::install::ModuleInstallOutcome::Skip { kind, .. }
                            | crate::install::ModuleInstallOutcome::Replace { kind, .. } => {
                                kind.clone()
                            }
                        };
                        self.push_action_message(crate::install::format_module_outcome(&outcome));
                        self.refresh_wasm_derived_state_for_kinds(&[kind]);
                    }
                    Err(err) => self.push_action_message(err.to_string()),
                }
            }
            InstallTarget::Extension => {
                let extensions_dir = self.extensions_dir.clone().expect("checked above");
                match crate::install::install_extension_into(&extensions_dir, source) {
                    Ok(outcome) => {
                        let replaced = matches!(
                            outcome,
                            crate::install::ExtensionInstallOutcome::Replace { .. }
                        );
                        self.push_action_message(crate::install::format_extension_outcome(
                            &outcome,
                        ));
                        if replaced {
                            self.push_action_message(
                                "restart may be needed for replaced extension".to_string(),
                            );
                        }
                        self.refresh_installed_extensions();
                    }
                    Err(err) => self.push_action_message(err.to_string()),
                }
            }
        }
    }

    pub(in crate::tui) fn extension_overlay_labels(&self) -> &[String] {
        &self.extension_overlay_labels
    }

    pub(in crate::tui) fn install_kind_labels() -> [&'static str; 2] {
        ["module", "extension"]
    }
}
