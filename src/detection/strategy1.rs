use crate::config::Strategy1Config;
use crate::detection::EpisodeTracker;
use crate::models::SymbolData;
use crate::utils::EpisodeLogger;
use std::sync::Arc;
use tracing::info;

pub struct Strategy1 {
    config: Strategy1Config,
    tracker: EpisodeTracker,
    logger: Arc<EpisodeLogger>,
}

impl Strategy1 {
    pub fn new(config: Strategy1Config, cooldown_seconds: u64, logger: Arc<EpisodeLogger>) -> Self {
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
        let abs_diff = last_price - mark_price;

        let condition_met = ratio >= self.config.spread_ratio_min
            && abs_diff >= self.config.min_abs_diff;

        let (episode_opt, started) = self.tracker.check_condition(
            &data.symbol,
            condition_met,
            ratio,
            last_price,
            mark_price,
        );

        // Log episode start
        if started {
            info!(
                "[Strategy1] 🚨 ANOMALY DETECTED: {} | Ratio: {:.4} | Last: {:.4} | Mark: {:.4}",
                data.symbol, ratio, last_price, mark_price
            );
        }

        // Log episode end
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
                    "[Strategy1] ✅ Episode ended: {} | Peak Ratio: {:.4} | Duration: {:?}",
                    episode.symbol, episode.peak_ratio,
                    chrono::Utc::now().signed_duration_since(episode.start_time)
                );
            }
        }
    }
}
