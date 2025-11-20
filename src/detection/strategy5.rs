use crate::config::{OrderbookConfig, Strategy1Config, Strategy2Config, Strategy3Config, Strategy4Config, Strategy5Config};
use crate::detection::EpisodeTracker;
use crate::models::SymbolData;
use crate::utils::EpisodeLogger;
use std::sync::Arc;
use tracing::info;

pub struct Strategy5 {
    config: Strategy5Config,
    strategy1_config: Strategy1Config,
    strategy2_config: Strategy2Config,
    strategy3_config: Strategy3Config,
    strategy4_config: Strategy4Config,
    orderbook_config: OrderbookConfig,
    tracker: EpisodeTracker,
    logger: Arc<EpisodeLogger>,
}

impl Strategy5 {
    pub fn new(
        config: Strategy5Config,
        strategy1_config: Strategy1Config,
        strategy2_config: Strategy2Config,
        strategy3_config: Strategy3Config,
        strategy4_config: Strategy4Config,
        orderbook_config: OrderbookConfig,
        cooldown_seconds: u64,
        logger: Arc<EpisodeLogger>,
    ) -> Self {
        Self {
            config,
            strategy1_config,
            strategy2_config,
            strategy3_config,
            strategy4_config,
            orderbook_config,
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

        // Check all 4 strategy conditions

        // Condition 1: Basic spread (Strategy 1)
        let abs_diff = last_price - mark_price;
        let condition1 = ratio >= self.strategy1_config.spread_ratio_min
            && abs_diff >= self.strategy1_config.min_abs_diff;

        if !condition1 {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Condition 2: Spike detection (Strategy 2)
        let historical_price = data.get_price_at(self.strategy2_config.spike_lookback_secs);
        let spike_ratio = match historical_price {
            Some(old_price) => last_price / old_price,
            None => {
                // Not enough history yet
                return;
            }
        };

        let condition2 = ratio >= self.strategy2_config.spread_ratio_min
            && spike_ratio >= self.strategy2_config.spike_ratio_min;

        if !condition2 {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Condition 3: Baseline stability (Strategy 3)
        let (baseline_last, baseline_mark) = match data.get_baseline_prices(self.strategy3_config.baseline_window_secs) {
            Some(prices) => prices,
            None => {
                // Not enough history yet
                return;
            }
        };

        let pump_ratio = last_price / baseline_last;
        let mark_deviation = (mark_price / baseline_mark - 1.0).abs();

        let condition3 = ratio >= self.strategy3_config.spread_ratio_min
            && pump_ratio >= self.strategy3_config.pump_vs_baseline_min
            && mark_deviation <= self.strategy3_config.mark_stability_max;

        if !condition3 {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        // Condition 4: Thick orderbook (Strategy 4)
        let orderbook = match &data.orderbook {
            Some(ob) => ob,
            None => {
                // No orderbook data yet
                return;
            }
        };

        let mid_price = match orderbook.calculate_mid_price() {
            Some(mid) => mid,
            None => return,
        };

        let spread_pct = match orderbook.calculate_spread_pct() {
            Some(spread) => spread,
            None => return,
        };

        if spread_pct > self.orderbook_config.max_spread_pct {
            self.tracker.check_condition(&data.symbol, false, ratio, last_price, mark_price);
            return;
        }

        let depth = orderbook.calculate_depth_in_band(
            mid_price,
            self.orderbook_config.depth_band_pct,
        );

        let condition4 = ratio >= self.strategy4_config.spread_ratio_min
            && abs_diff >= self.strategy4_config.min_abs_diff
            && depth >= self.orderbook_config.min_thick_depth_usdt;

        // ALL 4 conditions must be met
        let all_conditions_met = condition1 && condition2 && condition3 && condition4;

        let (episode_opt, started) = self.tracker.check_condition(
            &data.symbol,
            all_conditions_met,
            ratio,
            last_price,
            mark_price,
        );

        if started {
            info!(
                "[Strategy5] 🔥 CRITICAL ANOMALY: {} | Ratio: {:.4} | ALL 4 CONDITIONS MET | Spike: {:.2}x | Pump: {:.2}x | Depth: ${:.0}",
                data.symbol, ratio, spike_ratio, pump_ratio, depth
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
                    "[Strategy5] ✅ Critical episode ended: {} | Peak Ratio: {:.4} | Duration: {:?}",
                    episode.symbol, episode.peak_ratio,
                    chrono::Utc::now().signed_duration_since(episode.start_time)
                );
            }
        }
    }
}
