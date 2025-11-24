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

/// Represents a candlestick (OHLCV) for a specific time window
#[derive(Debug, Clone)]
pub struct Candle {
    pub timestamp_ms: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,  // Note: Currently set to 0.0 as volume not available in WebSocket data
}

impl Candle {
    pub fn from_single_price(timestamp: DateTime<Utc>, price: f64) -> Self {
        Self {
            timestamp_ms: timestamp.timestamp_millis(),
            open: price,
            high: price,
            low: price,
            close: price,
            volume: 0.0,
        }
    }

    pub fn update_price(&mut self, price: f64) {
        if price > self.high {
            self.high = price;
        }
        if price < self.low {
            self.low = price;
        }
        self.close = price;
    }
}

/// Accumulates price updates into 500ms candles
#[derive(Debug, Clone)]
pub struct CandleBuffer {
    window_ms: i64,
    current_window_start: Option<i64>,
    current_last_price_candle: Option<Candle>,
    current_mark_price_candle: Option<Candle>,
    completed_last_price_candles: VecDeque<Candle>,
    completed_mark_price_candles: VecDeque<Candle>,
    last_known_last_price: Option<f64>,
    last_known_mark_price: Option<f64>,
}

impl CandleBuffer {
    pub fn new(window_ms: i64) -> Self {
        Self {
            window_ms,
            current_window_start: None,
            current_last_price_candle: None,
            current_mark_price_candle: None,
            completed_last_price_candles: VecDeque::new(),
            completed_mark_price_candles: VecDeque::new(),
            last_known_last_price: None,
            last_known_mark_price: None,
        }
    }

    pub fn add_price_update(&mut self, last_price: Option<f64>, mark_price: Option<f64>, timestamp: DateTime<Utc>) {
        let ts_ms = timestamp.timestamp_millis();
        let window_start = (ts_ms / self.window_ms) * self.window_ms;

        // Check if we've moved to a new window
        if let Some(current_start) = self.current_window_start {
            if window_start > current_start {
                // Complete the current candles and start new ones
                self.complete_current_candles(current_start);

                // Forward-fill any gaps with last known prices
                let mut gap_start = current_start + self.window_ms;
                let mut gap_count = 0;
                while gap_start < window_start {
                    self.forward_fill_candle(gap_start);
                    gap_start += self.window_ms;
                    gap_count += 1;
                }
            }
        }

        self.current_window_start = Some(window_start);

        // Update last_price candle
        if let Some(price) = last_price {
            self.last_known_last_price = Some(price);
            match &mut self.current_last_price_candle {
                Some(candle) => candle.update_price(price),
                None => {
                    self.current_last_price_candle = Some(Candle::from_single_price(
                        DateTime::from_timestamp_millis(window_start).unwrap_or(timestamp),
                        price
                    ));
                }
            }
        }

        // Update mark_price candle
        if let Some(price) = mark_price {
            self.last_known_mark_price = Some(price);
            match &mut self.current_mark_price_candle {
                Some(candle) => candle.update_price(price),
                None => {
                    self.current_mark_price_candle = Some(Candle::from_single_price(
                        DateTime::from_timestamp_millis(window_start).unwrap_or(timestamp),
                        price
                    ));
                }
            }
        }
    }

    fn complete_current_candles(&mut self, _window_start: i64) {
        if let Some(candle) = self.current_last_price_candle.take() {
            self.completed_last_price_candles.push_back(candle);
        }
        if let Some(candle) = self.current_mark_price_candle.take() {
            self.completed_mark_price_candles.push_back(candle);
        }

        // Keep only last 20 seconds of completed candles (40 candles at 500ms each)
        while self.completed_last_price_candles.len() > 40 {
            self.completed_last_price_candles.pop_front();
        }
        while self.completed_mark_price_candles.len() > 40 {
            self.completed_mark_price_candles.pop_front();
        }
    }

    fn forward_fill_candle(&mut self, window_start: i64) {
        let timestamp = DateTime::from_timestamp_millis(window_start).unwrap_or_else(Utc::now);

        if let Some(price) = self.last_known_last_price {
            self.completed_last_price_candles.push_back(Candle::from_single_price(timestamp, price));
        }
        if let Some(price) = self.last_known_mark_price {
            self.completed_mark_price_candles.push_back(Candle::from_single_price(timestamp, price));
        }
    }

    pub fn get_recent_candles(&self, seconds: i64) -> (Vec<Candle>, Vec<Candle>) {
        let num_candles = (seconds * 1000 / self.window_ms) as usize;

        let last_price_candles: Vec<Candle> = self.completed_last_price_candles
            .iter()
            .rev()
            .take(num_candles)
            .rev()
            .cloned()
            .collect();

        let mark_price_candles: Vec<Candle> = self.completed_mark_price_candles
            .iter()
            .rev()
            .take(num_candles)
            .rev()
            .cloned()
            .collect();

        (last_price_candles, mark_price_candles)
    }

    pub fn get_all_completed_candles(&self) -> (Vec<Candle>, Vec<Candle>) {
        (
            self.completed_last_price_candles.iter().cloned().collect(),
            self.completed_mark_price_candles.iter().cloned().collect()
        )
    }

    pub fn get_pre_buffer_candles(&self, seconds: i64) -> (Vec<Candle>, Vec<Candle>) {
        let requested_count = (seconds * 1000 / self.window_ms) as usize;
        let all_candles = self.get_all_completed_candles();

        let last_len = all_candles.0.len();
        let mark_len = all_candles.1.len();

        let last_price_candles = if last_len > requested_count {
            all_candles.0.into_iter().skip(last_len - requested_count).collect()
        } else {
            all_candles.0
        };

        let mark_price_candles = if mark_len > requested_count {
            all_candles.1.into_iter().skip(mark_len - requested_count).collect()
        } else {
            all_candles.1
        };

        (last_price_candles, mark_price_candles)
    }
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

    // Candle buffer for CSV export
    pub candle_buffer: CandleBuffer,
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
            candle_buffer: CandleBuffer::new(500), // 500ms candles
        }
    }

    pub fn update_last_price(&mut self, price: f64, timestamp: DateTime<Utc>) {
        self.current_last_price = Some(price);
        self.last_update = timestamp;
        self.add_to_history();
        // Update candle buffer
        self.candle_buffer.add_price_update(Some(price), self.current_mark_price, timestamp);
    }

    pub fn update_mark_price(&mut self, price: f64, timestamp: DateTime<Utc>) {
        self.current_mark_price = Some(price);
        self.last_update = timestamp;
        self.add_to_history();
        // Update candle buffer
        self.candle_buffer.add_price_update(self.current_last_price, Some(price), timestamp);
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
