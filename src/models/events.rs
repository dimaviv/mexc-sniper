use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub enum MarketEvent {
    TickerUpdate {
        symbol: String,
        last_price: f64,
        mark_price: Option<f64>,
        timestamp: DateTime<Utc>,
    },
    MarkPriceUpdate {
        symbol: String,
        mark_price: f64,
        timestamp: DateTime<Utc>,
    },
    OrderbookUpdate {
        symbol: String,
        orderbook: super::ProcessedOrderbook,
    },
}
