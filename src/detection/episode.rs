use chrono::{DateTime, Utc};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct Episode {
    pub symbol: String,
    pub start_time: DateTime<Utc>,
    pub peak_ratio: f64,
    pub peak_last_price: f64,
    pub peak_mark_price: f64,
    pub last_cooldown_end: Option<DateTime<Utc>>,
}

impl Episode {
    pub fn new(symbol: String, ratio: f64, last_price: f64, mark_price: f64) -> Self {
        Self {
            symbol,
            start_time: Utc::now(),
            peak_ratio: ratio,
            peak_last_price: last_price,
            peak_mark_price: mark_price,
            last_cooldown_end: None,
        }
    }

    pub fn update_peak(&mut self, ratio: f64, last_price: f64, mark_price: f64) {
        if ratio > self.peak_ratio {
            self.peak_ratio = ratio;
            self.peak_last_price = last_price;
            self.peak_mark_price = mark_price;
        }
    }
}

pub struct EpisodeTracker {
    active_episodes: HashMap<String, Episode>,
    cooldown_seconds: u64,
}

impl EpisodeTracker {
    pub fn new(cooldown_seconds: u64) -> Self {
        Self {
            active_episodes: HashMap::new(),
            cooldown_seconds,
        }
    }

    pub fn check_condition(
        &mut self,
        symbol: &str,
        condition_met: bool,
        ratio: f64,
        last_price: f64,
        mark_price: f64,
    ) -> (Option<Episode>, bool) {
        if condition_met {
            if let Some(episode) = self.active_episodes.get_mut(symbol) {
                // Update existing episode
                episode.update_peak(ratio, last_price, mark_price);
                (None, false)
            } else {
                // Check if still in cooldown
                let now = Utc::now();
                if let Some(last_cooldown) = self.active_episodes
                    .get(symbol)
                    .and_then(|e| e.last_cooldown_end)
                {
                    if now < last_cooldown {
                        return (None, false);
                    }
                }

                // Start new episode
                let episode = Episode::new(symbol.to_string(), ratio, last_price, mark_price);
                self.active_episodes.insert(symbol.to_string(), episode);
                (None, true) // Return true to indicate episode started
            }
        } else {
            // Condition no longer met
            if let Some(mut episode) = self.active_episodes.remove(symbol) {
                // End episode and apply cooldown
                episode.last_cooldown_end = Some(Utc::now() + chrono::Duration::seconds(self.cooldown_seconds as i64));
                (Some(episode), false)
            } else {
                (None, false)
            }
        }
    }
}
