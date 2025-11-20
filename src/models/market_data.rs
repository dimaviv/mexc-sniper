use chrono::{DateTime, Utc};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::VecDeque;

// Helper function to deserialize string or number as string
fn string_or_number<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::String(s) => Ok(s),
        Value::Number(n) => Ok(n.to_string()),
        _ => Err(Error::custom("expected string or number")),
    }
}

// Helper function for optional string or number
fn option_string_or_number<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(None),
        Value::String(s) => Ok(Some(s)),
        Value::Number(n) => Ok(Some(n.to_string())),
        _ => Err(Error::custom("expected string, number, or null")),
    }
}

// Helper function for Vec<Vec<String>> where inner values can be strings or numbers
fn vec_vec_string_or_number<'de, D>(deserializer: D) -> Result<Vec<Vec<String>>, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    use serde_json::Value;

    let value = Value::deserialize(deserializer)?;
    match value {
        Value::Array(arr) => {
            let mut result = Vec::new();
            for item in arr {
                match item {
                    Value::Array(inner_arr) => {
                        let mut inner_result = Vec::new();
                        for inner_item in inner_arr {
                            let s = match inner_item {
                                Value::String(s) => s,
                                Value::Number(n) => n.to_string(),
                                _ => continue,
                            };
                            inner_result.push(s);
                        }
                        result.push(inner_result);
                    }
                    _ => continue,
                }
            }
            Ok(result)
        }
        _ => Err(Error::custom("expected array of arrays")),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickerData {
    pub symbol: String,
    #[serde(rename = "lastPrice", deserialize_with = "string_or_number")]
    pub last_price: String,
    #[serde(rename = "fairPrice", default, deserialize_with = "option_string_or_number")]
    pub fair_price: Option<String>,
    #[serde(rename = "bid1", default, deserialize_with = "option_string_or_number")]
    pub bid1: Option<String>,
    #[serde(rename = "ask1", default, deserialize_with = "option_string_or_number")]
    pub ask1: Option<String>,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkPriceData {
    pub symbol: String,
    #[serde(rename = "fairPrice", deserialize_with = "string_or_number")]
    pub fair_price: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderbookData {
    pub symbol: Option<String>,
    #[serde(deserialize_with = "vec_vec_string_or_number")]
    pub asks: Vec<Vec<String>>,
    #[serde(deserialize_with = "vec_vec_string_or_number")]
    pub bids: Vec<Vec<String>>,
    #[serde(default = "default_timestamp")]
    pub timestamp: i64,
}

fn default_timestamp() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[derive(Debug, Clone)]
pub struct OrderbookLevel {
    pub price: f64,
    pub quantity: f64,
}

#[derive(Debug, Clone)]
pub struct ProcessedOrderbook {
    pub bids: Vec<OrderbookLevel>,
    pub asks: Vec<OrderbookLevel>,
    pub timestamp: DateTime<Utc>,
}

impl ProcessedOrderbook {
    pub fn from_raw(raw: &OrderbookData, max_levels: usize) -> Self {
        let bids = raw.bids.iter()
            .take(max_levels)
            .filter_map(|level| {
                if level.len() >= 2 {
                    let price = level[0].parse::<f64>().ok()?;
                    let quantity = level[1].parse::<f64>().ok()?;
                    Some(OrderbookLevel { price, quantity })
                } else {
                    None
                }
            })
            .collect();

        let asks = raw.asks.iter()
            .take(max_levels)
            .filter_map(|level| {
                if level.len() >= 2 {
                    let price = level[0].parse::<f64>().ok()?;
                    let quantity = level[1].parse::<f64>().ok()?;
                    Some(OrderbookLevel { price, quantity })
                } else {
                    None
                }
            })
            .collect();

        let timestamp = DateTime::from_timestamp_millis(raw.timestamp)
            .unwrap_or_else(Utc::now);

        ProcessedOrderbook {
            bids,
            asks,
            timestamp,
        }
    }

    pub fn calculate_mid_price(&self) -> Option<f64> {
        let best_bid = self.bids.first()?.price;
        let best_ask = self.asks.first()?.price;
        Some((best_bid + best_ask) / 2.0)
    }

    pub fn calculate_spread_pct(&self) -> Option<f64> {
        let best_bid = self.bids.first()?.price;
        let best_ask = self.asks.first()?.price;
        let mid = (best_bid + best_ask) / 2.0;
        Some((best_ask - best_bid) / mid)
    }

    pub fn calculate_depth_in_band(&self, mid_price: f64, band_pct: f64) -> f64 {
        let lower = mid_price * (1.0 - band_pct);
        let upper = mid_price * (1.0 + band_pct);

        let bid_depth: f64 = self.bids.iter()
            .filter(|level| level.price >= lower)
            .map(|level| level.price * level.quantity)
            .sum();

        let ask_depth: f64 = self.asks.iter()
            .filter(|level| level.price <= upper)
            .map(|level| level.price * level.quantity)
            .sum();

        bid_depth + ask_depth
    }
}

#[derive(Debug, Clone)]
pub struct PriceSnapshot {
    pub last_price: f64,
    pub mark_price: f64,
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SymbolData {
    pub symbol: String,
    pub current_last_price: Option<f64>,
    pub current_mark_price: Option<f64>,
    pub orderbook: Option<ProcessedOrderbook>,
    pub last_update: DateTime<Utc>,

    // Historical data for strategies
    pub price_history: VecDeque<PriceSnapshot>,
}

impl SymbolData {
    pub fn new(symbol: String) -> Self {
        Self {
            symbol,
            current_last_price: None,
            current_mark_price: None,
            orderbook: None,
            last_update: Utc::now(),
            price_history: VecDeque::new(),
        }
    }

    pub fn update_last_price(&mut self, price: f64, timestamp: DateTime<Utc>) {
        self.current_last_price = Some(price);
        self.last_update = timestamp;
        self.add_to_history();
    }

    pub fn update_mark_price(&mut self, price: f64, timestamp: DateTime<Utc>) {
        self.current_mark_price = Some(price);
        self.last_update = timestamp;
        self.add_to_history();
    }

    pub fn update_orderbook(&mut self, orderbook: ProcessedOrderbook) {
        self.orderbook = Some(orderbook);
        self.last_update = Utc::now();
    }

    fn add_to_history(&mut self) {
        if let (Some(last), Some(mark)) = (self.current_last_price, self.current_mark_price) {
            let snapshot = PriceSnapshot {
                last_price: last,
                mark_price: mark,
                timestamp: self.last_update,
            };

            self.price_history.push_back(snapshot);

            // Keep only last 2 minutes of history
            let cutoff = Utc::now() - chrono::Duration::seconds(120);
            while let Some(front) = self.price_history.front() {
                if front.timestamp < cutoff {
                    self.price_history.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    pub fn get_price_at(&self, seconds_ago: u64) -> Option<f64> {
        let target_time = Utc::now() - chrono::Duration::seconds(seconds_ago as i64);

        self.price_history.iter()
            .filter(|s| s.timestamp <= target_time)
            .last()
            .map(|s| s.last_price)
    }

    pub fn get_baseline_prices(&self, window_secs: u64) -> Option<(f64, f64)> {
        let cutoff = Utc::now() - chrono::Duration::seconds(window_secs as i64);

        let relevant: Vec<_> = self.price_history.iter()
            .filter(|s| s.timestamp >= cutoff)
            .collect();

        if relevant.is_empty() {
            return None;
        }

        let avg_last: f64 = relevant.iter().map(|s| s.last_price).sum::<f64>() / relevant.len() as f64;
        let avg_mark: f64 = relevant.iter().map(|s| s.mark_price).sum::<f64>() / relevant.len() as f64;

        Some((avg_last, avg_mark))
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractDetail {
    pub symbol: String,
    #[serde(rename = "displayName")]
    pub display_name: String,
    pub state: i32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ContractDetailResponse {
    pub success: bool,
    pub code: i32,
    pub data: Vec<ContractDetail>,
}
