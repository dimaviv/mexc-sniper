use crate::config::{OrderbookConfig, Strategy4Config};
use crate::detection::EpisodeTracker;
use crate::export::CsvExporter;
use crate::models::SymbolData;
use crate::utils::EpisodeLogger;
use std::sync::Arc;
use tracing::info;

pub struct Strategy4 {
    config: Strategy4Config,
    orderbook_config: OrderbookConfig,
    tracker: EpisodeTracker,
    logger: Arc<EpisodeLogger>,
    csv_exporter: Option<Arc<CsvExporter>>,
    pre_buffer_secs: i64,
}

impl Strategy4 {
    pub fn new(
        config: Strategy4Config,
        orderbook_config: OrderbookConfig,
        cooldown_seconds: u64,
        logger: Arc<EpisodeLogger>,
        csv_exporter: Option<Arc<CsvExporter>>,
        pre_buffer_secs: i64,
    ) -> Self {
        Self {
            config,
            orderbook_config,
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

        // Check base spread conditions (like Strategy1)
        if ratio < self.config.spread_ratio_min || abs_diff < self.config.min_abs_diff {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Check orderbook conditions
        let orderbook = match &data.orderbook {
            Some(ob) => ob,
            None => {
                // No orderbook data yet
                return;
            }
        };

        // Calculate mid price
        let mid_price = match orderbook.calculate_mid_price() {
            Some(mid) => mid,
            None => return,
        };

        // Check spread
        let spread_pct = match orderbook.calculate_spread_pct() {
            Some(spread) => spread,
            None => return,
        };

        if spread_pct > self.orderbook_config.max_spread_pct {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Check depth in band
        let depth = orderbook.calculate_depth_in_band(
            mid_price,
            self.orderbook_config.depth_band_pct,
        );

        let condition_met = depth >= self.orderbook_config.min_thick_depth_usdt;

        let (episode_opt, started) = self.tracker.check_condition(
            &data.symbol,
            condition_met,
            ratio,
            last_price,
            mark_price,
        );

        if started {
            info!(
                "[Strategy4] 🚨 ANOMALY DETECTED: {} | Ratio: {:.4} | Thick Book: ${:.0}",
                data.symbol, ratio, depth
            );

            if let Some(ref exporter) = self.csv_exporter {
                let pre_buffer_candles = data.candle_buffer.get_pre_buffer_candles(self.pre_buffer_secs);
                exporter.start_recording(&data.symbol, "strategy4", pre_buffer_candles);
            }
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
                    "[Strategy4] ✅ Episode ended: {} | Peak Ratio: {:.4}",
                    episode.symbol, episode.peak_ratio
                );

                if let Some(ref exporter) = self.csv_exporter {
                    exporter.mark_anomaly_ended(&episode.symbol, "strategy4");
                }
            }
        }
    }
}
