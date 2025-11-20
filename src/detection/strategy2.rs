use crate::config::Strategy2Config;
use crate::detection::EpisodeTracker;
use crate::models::SymbolData;
use crate::utils::EpisodeLogger;
use std::sync::Arc;
use tracing::info;

pub struct Strategy2 {
    config: Strategy2Config,
    tracker: EpisodeTracker,
    logger: Arc<EpisodeLogger>,
}

impl Strategy2 {
    pub fn new(config: Strategy2Config, cooldown_seconds: u64, logger: Arc<EpisodeLogger>) -> Self {
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
            // Condition not met, check for episode end
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Check spike condition
        let historical_price = data.get_price_at(self.config.spike_lookback_secs);
        let spike_ratio = match historical_price {
            Some(old_price) => last_price / old_price,
            None => {
                // Not enough history yet
                return;
            }
        };

        let condition_met = spike_ratio >= self.config.spike_ratio_min;

        let (episode_opt, started) = self.tracker.check_condition(
            &data.symbol,
            condition_met,
            ratio,
            last_price,
            mark_price,
        );

        if started {
            info!(
                "[Strategy2] 🚨 ANOMALY DETECTED: {} | Ratio: {:.4} | Spike: {:.4}x",
                data.symbol, ratio, spike_ratio
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
                    "[Strategy2] ✅ Episode ended: {} | Peak Ratio: {:.4}",
                    episode.symbol, episode.peak_ratio
                );
            }
        }
    }
}
