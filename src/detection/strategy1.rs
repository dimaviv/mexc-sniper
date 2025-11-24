use crate::config::Strategy1Config;
use crate::detection::EpisodeTracker;
use crate::export::CsvExporter;
use crate::models::SymbolData;
use crate::utils::EpisodeLogger;
use std::sync::Arc;
use tracing::info;

pub struct Strategy1 {
    config: Strategy1Config,
    tracker: EpisodeTracker,
    logger: Arc<EpisodeLogger>,
    csv_exporter: Option<Arc<CsvExporter>>,
    pre_buffer_secs: i64,
}

impl Strategy1 {
    pub fn new(
        config: Strategy1Config,
        cooldown_seconds: u64,
        logger: Arc<EpisodeLogger>,
        csv_exporter: Option<Arc<CsvExporter>>,
        pre_buffer_secs: i64,
    ) -> Self {
        Self {
            config,
            tracker: EpisodeTracker::new(cooldown_seconds),
            logger,
            csv_exporter,
            pre_buffer_secs,
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

        // Log episode start and start CSV recording
        if started {
            info!(
                "[Strategy1] 🚨 ANOMALY DETECTED: {} | Ratio: {:.4} | Last: {:.4} | Mark: {:.4}",
                data.symbol, ratio, last_price, mark_price
            );

            // Start CSV recording if exporter is available
            info!("[Strategy1] Checking if CSV exporter is available...");
            if let Some(ref exporter) = self.csv_exporter {
                info!("[Strategy1] CSV exporter found - getting pre-buffer candles from SymbolData");
                // Get pre-buffer candles from the current SymbolData (no lock needed, already have it)
                let pre_buffer_candles = data.candle_buffer.get_pre_buffer_candles(self.pre_buffer_secs);
                info!("[Strategy1] Got {} last_price and {} mark_price candles",
                    pre_buffer_candles.0.len(), pre_buffer_candles.1.len());

                info!("[Strategy1] Calling start_recording()");
                exporter.start_recording(&data.symbol, "strategy1", pre_buffer_candles);
                info!("[Strategy1] start_recording() call completed");
            } else {
                info!("[Strategy1] CSV exporter is NOT available (None)");
            }
        }

        // Log episode end and mark anomaly ended for CSV recording
        if let Some(episode) = episode_opt {
            info!("[Strategy1] Episode ended detected for {}", episode.symbol);

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

                // Mark anomaly ended for CSV recording
                info!("[Strategy1] Checking if CSV exporter is available for mark_anomaly_ended...");
                if let Some(ref exporter) = self.csv_exporter {
                    info!("[Strategy1] CSV exporter found - calling mark_anomaly_ended()");
                    exporter.mark_anomaly_ended(&episode.symbol, "strategy1");
                    info!("[Strategy1] mark_anomaly_ended() call completed");
                } else {
                    info!("[Strategy1] CSV exporter is NOT available (None)");
                }
            }
        }
    }
}
