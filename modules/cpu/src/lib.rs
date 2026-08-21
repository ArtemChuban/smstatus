wit_bindgen::generate!({
    path: "../../wit",
    world: "module",
    additional_derives: [PartialEq],
});

use crate::smstatus::module::host;
use exports::smstatus::module::guest::{ConfigParam, Guest, HostApiVersion, Output};
use logic::CpuTimes;
use serde::Deserialize;
use std::cell::RefCell;

const DEFAULT_PATH: &str = "/proc/stat";
const DEFAULT_FORMAT: &str = "CPU {usage:3}%";
const REQUIRED_HOST_API: (u32, u32, u32) = (1, 0, 0);

#[derive(Deserialize, Default, Debug, PartialEq)]
struct Config {
    path: Option<String>,
    format: Option<String>,
}

thread_local! {
    static PATH: RefCell<String> = RefCell::new(DEFAULT_PATH.to_string());
    static FORMAT: RefCell<String> = RefCell::new(DEFAULT_FORMAT.to_string());
    static PREV: RefCell<Option<CpuTimes>> = const { RefCell::new(None) };
}

mod logic {
    use super::Config;

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct CpuTimes {
        pub user: u64,
        pub nice: u64,
        pub system: u64,
        pub idle: u64,
        pub iowait: u64,
        pub irq: u64,
        pub softirq: u64,
        pub steal: u64,
    }

    impl CpuTimes {
        fn total(&self) -> u64 {
            self.user
                + self.nice
                + self.system
                + self.idle
                + self.iowait
                + self.irq
                + self.softirq
                + self.steal
        }

        fn idle_total(&self) -> u64 {
            self.idle + self.iowait
        }
    }

    pub fn parse_config(config: &str) -> Option<Config> {
        serde_json::from_str::<Config>(config).ok()
    }

    pub fn parse_cpu_line(content: &str) -> Option<CpuTimes> {
        let line = content.lines().find(|line| line.starts_with("cpu "))?;
        let mut fields = line.split_whitespace().skip(1);
        let mut next = || fields.next()?.parse::<u64>().ok();
        Some(CpuTimes {
            user: next()?,
            nice: next()?,
            system: next()?,
            idle: next()?,
            iowait: next()?,
            irq: next()?,
            softirq: next()?,
            steal: next()?,
        })
    }

    pub fn usage_percent(prev: &CpuTimes, curr: &CpuTimes) -> f64 {
        let total_delta = curr.total().saturating_sub(prev.total());
        if total_delta == 0 {
            return 0.0;
        }
        let idle_delta = curr.idle_total().saturating_sub(prev.idle_total());
        100.0 * (1.0 - idle_delta as f64 / total_delta as f64)
    }

    pub fn format_cpu(format: &str, percent: f64) -> String {
        let usage = format!("{percent:.0}");
        fmt_common::format_template(format, &[("usage", &usage)])
    }

    pub fn format_error(err: &str) -> String {
        format!("cpu error: {err}")
    }
}

struct Component;

impl Guest for Component {
    fn init(config: String) {
        if let Some(parsed) = logic::parse_config(&config) {
            let path = parsed.path.unwrap_or_else(|| DEFAULT_PATH.to_string());
            let format = parsed.format.unwrap_or_else(|| DEFAULT_FORMAT.to_string());
            PATH.with(|p| *p.borrow_mut() = path);
            FORMAT.with(|f| *f.borrow_mut() = format);
        }
    }

    fn update() -> Output {
        let path = PATH.with(|p| p.borrow().clone());
        let text = match host::read_sysfs(&path) {
            Ok(content) => match logic::parse_cpu_line(&content) {
                Some(curr) => {
                    let prev = PREV.with(|p| p.borrow_mut().replace(curr));
                    match prev {
                        Some(prev) => FORMAT.with(|f| {
                            logic::format_cpu(&f.borrow(), logic::usage_percent(&prev, &curr))
                        }),
                        None => FORMAT.with(|f| logic::format_cpu(&f.borrow(), 0.0)),
                    }
                }
                None => logic::format_error(&format!("no `cpu ` line found in `{path}`")),
            },
            Err(err) => logic::format_error(&err),
        };
        Output {
            text,
            interval_ms: 2_000,
        }
    }

    fn required_host_api_version() -> HostApiVersion {
        let (major, minor, patch) = REQUIRED_HOST_API;
        HostApiVersion {
            major,
            minor,
            patch,
        }
    }

    fn config_schema() -> Vec<ConfigParam> {
        fmt_common::config_schema![
            ConfigParam,
            ("path", DEFAULT_PATH),
            ("format", DEFAULT_FORMAT),
        ]
    }
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::Config;
    use super::Guest;
    use super::logic::*;

    fn times(user: u64, nice: u64, system: u64, idle: u64, iowait: u64) -> CpuTimes {
        CpuTimes {
            user,
            nice,
            system,
            idle,
            iowait,
            irq: 0,
            softirq: 0,
            steal: 0,
        }
    }

    #[test]
    fn parses_valid_config_with_both_fields() {
        assert_eq!(
            parse_config(r#"{"path":"/proc/stat","format":"CPU {usage}%"}"#),
            Some(Config {
                path: Some("/proc/stat".to_string()),
                format: Some("CPU {usage}%".to_string()),
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_path() {
        assert_eq!(
            parse_config(r#"{"path":"/proc/stat"}"#),
            Some(Config {
                path: Some("/proc/stat".to_string()),
                format: None,
            })
        );
    }

    #[test]
    fn parses_valid_config_with_only_format() {
        assert_eq!(
            parse_config(r#"{"format":"{usage}%"}"#),
            Some(Config {
                path: None,
                format: Some("{usage}%".to_string()),
            })
        );
    }

    #[test]
    fn empty_json_object_yields_both_fields_none() {
        assert_eq!(
            parse_config("{}"),
            Some(Config {
                path: None,
                format: None,
            })
        );
    }

    #[test]
    fn returns_none_for_invalid_json() {
        assert_eq!(parse_config("not json"), None);
    }

    #[test]
    fn returns_none_for_empty_string() {
        assert_eq!(parse_config(""), None);
    }

    #[test]
    fn parses_aggregate_cpu_line() {
        let content = "cpu  100 20 30 800 50 0 0 0\ncpu0 50 10 15 400 25 0 0 0\n";
        assert_eq!(parse_cpu_line(content), Some(times(100, 20, 30, 800, 50)));
    }

    #[test]
    fn ignores_per_core_lines_and_finds_aggregate() {
        let content = "cpu0 50 10 15 400 25 0 0 0\ncpu  100 20 30 800 50 0 0 0\n";
        assert_eq!(parse_cpu_line(content), Some(times(100, 20, 30, 800, 50)));
    }

    #[test]
    fn returns_none_when_no_aggregate_line_present() {
        let content = "cpu0 50 10 15 400 25 0 0 0\n";
        assert_eq!(parse_cpu_line(content), None);
    }

    #[test]
    fn returns_none_for_too_few_fields() {
        let content = "cpu  100 20 30\n";
        assert_eq!(parse_cpu_line(content), None);
    }

    #[test]
    fn returns_none_for_malformed_numbers() {
        let content = "cpu  abc 20 30 800 50 0 0 0\n";
        assert_eq!(parse_cpu_line(content), None);
    }

    #[test]
    fn usage_percent_typical_delta() {
        let prev = times(100, 0, 0, 800, 0);
        let curr = times(200, 0, 0, 900, 0);
        assert_eq!(usage_percent(&prev, &curr), 50.0);
    }

    #[test]
    fn usage_percent_fully_idle() {
        let prev = times(0, 0, 0, 100, 0);
        let curr = times(0, 0, 0, 200, 0);
        assert_eq!(usage_percent(&prev, &curr), 0.0);
    }

    #[test]
    fn usage_percent_fully_busy() {
        let prev = times(0, 0, 0, 100, 0);
        let curr = times(100, 0, 0, 100, 0);
        assert_eq!(usage_percent(&prev, &curr), 100.0);
    }

    #[test]
    fn usage_percent_zero_total_delta_returns_zero() {
        let prev = times(100, 0, 0, 800, 0);
        assert_eq!(usage_percent(&prev, &prev), 0.0);
    }

    #[test]
    fn usage_percent_counts_iowait_as_idle() {
        let prev = times(0, 0, 0, 0, 0);
        let curr = times(0, 0, 0, 0, 100);
        assert_eq!(usage_percent(&prev, &curr), 0.0);
    }

    #[test]
    fn format_cpu_substitutes_placeholder_rounded() {
        assert_eq!(format_cpu("CPU {usage}%", 42.6), "CPU 43%");
    }

    #[test]
    fn format_cpu_no_placeholder_returns_unchanged() {
        assert_eq!(format_cpu("static text", 50.0), "static text");
    }

    #[test]
    fn format_cpu_space_pads_usage() {
        assert_eq!(format_cpu("CPU {usage:3}%", 5.0), "CPU   5%");
    }

    #[test]
    fn format_cpu_zero_pads_usage() {
        assert_eq!(format_cpu("CPU {usage:03}%", 5.0), "CPU 005%");
    }

    #[test]
    fn formats_simple_error_message() {
        assert_eq!(
            format_error("no `cpu ` line found in `/proc/stat`"),
            "cpu error: no `cpu ` line found in `/proc/stat`"
        );
    }

    #[test]
    fn formats_empty_error_message() {
        assert_eq!(format_error(""), "cpu error: ");
    }

    #[test]
    fn config_schema_declares_path_and_format() {
        assert_eq!(
            super::Component::config_schema(),
            vec![
                super::ConfigParam {
                    name: "path".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_PATH.to_string(),
                },
                super::ConfigParam {
                    name: "format".to_string(),
                    param_type: "string".to_string(),
                    default: super::DEFAULT_FORMAT.to_string(),
                },
            ]
        );
    }
}
