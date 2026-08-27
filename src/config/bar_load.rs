use std::path::Path;

use super::{BarConfig, active_config_path};

pub(crate) const IDLE_STATUS_MESSAGE: &str = "modules not configured";

pub(crate) enum BarConfigLoad {
    Ready { config: BarConfig },
    Idle,
}

pub(crate) fn load_bar_config(config_dir: &Path) -> BarConfigLoad {
    let path = match active_config_path(config_dir) {
        Ok(path) => path,
        Err(err) => {
            log::warn!("{err}");
            return BarConfigLoad::Idle;
        }
    };

    let config = match BarConfig::load(&path) {
        Ok(config) => config,
        Err(err) => {
            log::warn!("{err}");
            return BarConfigLoad::Idle;
        }
    };

    let module_names = match config.module_names() {
        Ok(names) => names,
        Err(err) => {
            log::warn!("{err}");
            return BarConfigLoad::Idle;
        }
    };

    if module_names.is_empty() {
        log::warn!("{IDLE_STATUS_MESSAGE}");
        return BarConfigLoad::Idle;
    }

    BarConfigLoad::Ready { config }
}
