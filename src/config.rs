use serde::Deserialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub api: ApiConfig,
    pub general: GeneralConfig,
    pub cooldowns: CooldownConfig,
    pub orderbook: OrderbookConfig,
    pub strategy1: Strategy1Config,
    pub strategy2: Strategy2Config,
    pub strategy3: Strategy3Config,
    pub strategy4: Strategy4Config,
    pub strategy5: Strategy5Config,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    pub base_rest_url: String,
    pub base_ws_url: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GeneralConfig {
    pub symbols: Vec<String>,
    pub log_dir: String,
    pub poll_interval_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CooldownConfig {
    pub per_symbol_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct OrderbookConfig {
    pub max_levels: usize,
    pub depth_band_pct: f64,
    pub min_thick_depth_usdt: f64,
    pub max_spread_pct: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Strategy1Config {
    pub enabled: bool,
    pub spread_ratio_min: f64,
    pub min_abs_diff: f64,
    pub min_price: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Strategy2Config {
    pub enabled: bool,
    pub spread_ratio_min: f64,
    pub spike_lookback_secs: u64,
    pub spike_ratio_min: f64,
    pub min_price: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Strategy3Config {
    pub enabled: bool,
    pub spread_ratio_min: f64,
    pub baseline_window_secs: u64,
    pub pump_vs_baseline_min: f64,
    pub mark_stability_max: f64,
    pub min_price: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Strategy4Config {
    pub enabled: bool,
    pub spread_ratio_min: f64,
    pub min_abs_diff: f64,
    pub min_price: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Strategy5Config {
    pub enabled: bool,
    pub min_price: f64,
}

impl Config {
    pub fn load<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let contents = fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }
}
