use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    #[default]
    Process,
}

impl CaptureMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            CaptureMode::Process => "process",
        }
    }

    pub fn parse(s: &str) -> Self {
        let _ = s;
        CaptureMode::Process
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StripSlot {
    #[default]
    WidgetsStart,
    NearTray,
}

impl StripSlot {
    pub fn as_str(&self) -> &'static str {
        match self {
            StripSlot::WidgetsStart => "widgets_start",
            StripSlot::NearTray => "near_tray",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "near_tray" => StripSlot::NearTray,
            _ => StripSlot::WidgetsStart,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum AppTheme {
    #[default]
    Midnight,
    Light,
    Nord,
    Dracula,
    TokyoNight,
}

impl AppTheme {
    pub fn as_str(&self) -> &'static str {
        match self {
            AppTheme::Midnight => "midnight",
            AppTheme::Light => "light",
            AppTheme::Nord => "nord",
            AppTheme::Dracula => "dracula",
            AppTheme::TokyoNight => "tokyo-night",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "light" => AppTheme::Light,
            "nord" => AppTheme::Nord,
            "dracula" => AppTheme::Dracula,
            "tokyo-night" => AppTheme::TokyoNight,
            _ => AppTheme::Midnight,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub capture_mode: CaptureMode,
    pub ping_targets: Vec<String>,
    pub http_targets: Vec<String>,
    pub ping_interval_secs: u64,
    pub http_interval_secs: u64,
    pub bandwidth_interval_ms: u64,
    pub retention_raw_days: u32,
    pub start_ui_with_windows: bool,
    pub speedtest_cli_path: Option<String>,
    pub speedtest_eula_accepted: bool,
    pub default_nic_id: Option<String>,
    /// Preferred taskbar speed-strip width in physical pixels (clamped 80–200).
    #[serde(default = "default_strip_width_px")]
    pub strip_width_px: u32,
    /// Speed-strip font size in CSS pixels (clamped 8–14).
    #[serde(default = "default_strip_font_px")]
    pub strip_font_px: u32,
    /// Preferred taskbar slot for the speed strip.
    #[serde(default)]
    pub strip_slot: StripSlot,
    /// Horizontal nudge in physical pixels (positive = right). Clamped −40..200.
    #[serde(default)]
    pub strip_offset_px: i32,
    #[serde(default)]
    pub ignore_private_ips: bool,
    #[serde(default)]
    pub ignored_apps: Vec<String>,
    #[serde(default)]
    pub ignored_ips: Vec<String>,
    #[serde(default)]
    pub ignored_urls: Vec<String>,
    #[serde(default)]
    pub theme: AppTheme,
    /// Live snapshot refresh interval for system metrics (CPU/RAM/disk).
    #[serde(default = "default_system_interval_ms")]
    pub system_interval_ms: u64,
    /// How often system metrics are written to SQLite.
    #[serde(default = "default_system_persist_interval_ms")]
    pub system_persist_interval_ms: u64,
    #[serde(default = "default_system_raw_retention_days")]
    pub system_raw_retention_days: u32,
    #[serde(default = "default_system_hourly_retention_days")]
    pub system_hourly_retention_days: u32,
}

fn default_strip_width_px() -> u32 {
    110
}

fn default_strip_font_px() -> u32 {
    10
}

fn default_system_interval_ms() -> u64 {
    2000
}

fn default_system_persist_interval_ms() -> u64 {
    5000
}

fn default_system_raw_retention_days() -> u32 {
    3
}

fn default_system_hourly_retention_days() -> u32 {
    90
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            capture_mode: CaptureMode::Process,
            ping_targets: vec!["8.8.8.8".into(), "1.1.1.1".into()],
            http_targets: vec![
                "https://www.cloudflare.com/cdn-cgi/trace".into(),
                "https://www.google.com/generate_204".into(),
            ],
            ping_interval_secs: 5,
            http_interval_secs: 30,
            bandwidth_interval_ms: 1000,
            retention_raw_days: 14,
            start_ui_with_windows: false,
            speedtest_cli_path: None,
            speedtest_eula_accepted: false,
            default_nic_id: None,
            strip_width_px: default_strip_width_px(),
            strip_font_px: default_strip_font_px(),
            strip_slot: StripSlot::WidgetsStart,
            strip_offset_px: 0,
            ignore_private_ips: false,
            ignored_apps: Vec::new(),
            ignored_ips: Vec::new(),
            ignored_urls: Vec::new(),
            theme: AppTheme::Midnight,
            system_interval_ms: default_system_interval_ms(),
            system_persist_interval_ms: default_system_persist_interval_ms(),
            system_raw_retention_days: default_system_raw_retention_days(),
            system_hourly_retention_days: default_system_hourly_retention_days(),
        }
    }
}

impl AppSettings {
    pub fn clamp_strip_prefs(&mut self) {
        self.strip_width_px = self.strip_width_px.clamp(80, 200);
        self.strip_font_px = self.strip_font_px.clamp(8, 14);
        self.strip_offset_px = self.strip_offset_px.clamp(-40, 200);
        self.system_interval_ms = self.system_interval_ms.max(500);
        self.system_persist_interval_ms = self.system_persist_interval_ms.max(1000);
        self.system_raw_retention_days = self.system_raw_retention_days.max(1);
        self.system_hourly_retention_days = self.system_hourly_retention_days.max(1);
    }
}
