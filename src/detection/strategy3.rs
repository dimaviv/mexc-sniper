use crate::config::Strategy3Config;
use crate::detection::EpisodeTracker;
use crate::models::SymbolData;
use crate::utils::EpisodeLogger;
use std::sync::Arc;
use tracing::info;

pub struct Strategy3 {
    config: Strategy3Config,
    tracker: EpisodeTracker,
    logger: Arc<EpisodeLogger>,
}

impl Strategy3 {
    pub fn new(config: Strategy3Config, cooldown_seconds: u64, logger: Arc<EpisodeLogger>) -> Self {
        Self {
            config,
            tracker: EpisodeTracker::new(cooldown_seconds),
            logger,
        }
    }

    pub fn check(&mut self, data: &SymbolData) {
        if !self.config.enabled {
            return;
        }

        let (last_price, mark_price) = match (data.current_last_price, data.current_mark_price) {
            (Some(l), Some(m)) => (l, m),
            _ => return,
        };

        if last_price < self.config.min_price {
            return;
        }

        let ratio = last_price / mark_price;

        // Check base spread condition
        if ratio < self.config.spread_ratio_min {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Get baseline averages
        let (baseline_last, baseline_mark) = match data.get_baseline_prices(self.config.baseline_window_secs) {
            Some(prices) => prices,
            None => {
                // Not enough history yet
                return;
            }
        };

        // Check pump vs baseline
        let pump_ratio = last_price / baseline_last;
        if pump_ratio < self.config.pump_vs_baseline_min {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Check mark stability
        let mark_deviation = (mark_price / baseline_mark - 1.0).abs();
        let condition_met = mark_deviation <= self.config.mark_stability_max;

        let (episode_opt, started) = self.tracker.check_condition(
            &data.symbol,
            condition_met,
            ratio,
            last_price,
            mark_price,
        );

        if started {
            info!(
                "[Strategy3] 🚨 ANOMALY DETECTED: {} | Ratio: {:.4} | Pump: {:.2}x baseline",
                data.symbol, ratio, last_price / baseline_last
            );
        }

        if let Some(episode) = episode_opt {
            if let Err(e) = self.logger.log_episode(
                &episode.symbol,
                episode.start_time,
                chrono::Utc::now(),
                episode.peak_ratio,
                episode.peak_last_price,
                episode.peak_mark_price,
            ) {
                tracing::error!("Failed to log episode: {:?}", e);
            } else {
                info!(
                    "[Strategy3] ✅ Episode ended: {} | Peak Ratio: {:.4}",
                    episode.symbol, episode.peak_ratio
                );
            }
        }
    }
}
