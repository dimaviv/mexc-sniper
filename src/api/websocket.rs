use crate::models::{MarketEvent, MarkPriceData, OrderbookData, ProcessedOrderbook, TickerData};
use anyhow::Result;
use chrono::{DateTime, Utc};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration, interval};
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

pub struct MexcWebSocketClient {
    ws_url: String,
    symbols: Vec<String>,
    max_levels: usize,
}

impl MexcWebSocketClient {
    pub fn new(ws_url: String, symbols: Vec<String>, max_levels: usize) -> Self {
        Self {
            ws_url,
            symbols,
            max_levels,
        }
    }

    pub async fn run(self, event_tx: mpsc::UnboundedSender<MarketEvent>) -> Result<()> {
        let mut reconnect_delay = Duration::from_secs(1);
        let max_reconnect_delay = Duration::from_secs(60);

        loop {
            info!("Connecting to WebSocket: {}", self.ws_url);

            match self.connect_and_run(&event_tx).await {
                Ok(_) => {
                    warn!("WebSocket connection closed normally");
                }
                Err(e) => {
                    error!("WebSocket error: {:?}", e);
                }
            }

            info!("Reconnecting in {:?}...", reconnect_delay);
            sleep(reconnect_delay).await;

            reconnect_delay = std::cmp::min(reconnect_delay * 2, max_reconnect_delay);
        }
    }

    async fn connect_and_run(&self, event_tx: &mpsc::UnboundedSender<MarketEvent>) -> Result<()> {
        let (ws_stream, _) = connect_async(&self.ws_url).await?;
        info!("WebSocket connected successfully");

        let (write, read) = ws_stream.split();

        // Create channels for write operations
        let (write_tx, mut write_rx) = mpsc::unbounded_channel::<Message>();

        // Spawn write task
        let write_handle = tokio::spawn(async move {
            let mut write = write;
            while let Some(msg) = write_rx.recv().await {
                if let Err(e) = write.send(msg).await {
                    error!("Failed to send message: {:?}", e);
                    break;
                }
            }
        });

        // Subscribe to ticker, mark price, and orderbook for each symbol
        for symbol in &self.symbols {
            // Subscribe to ticker for this symbol
            let ticker_sub = json!({
                "method": "sub.ticker",
                "param": {
                    "symbol": symbol
                }
            });
            write_tx.send(Message::Text(ticker_sub.to_string()))?;

            // Subscribe to fair/mark price for this symbol
            let mark_price_sub = json!({
                "method": "sub.fair_price",
                "param": {
                    "symbol": symbol
                }
            });
            write_tx.send(Message::Text(mark_price_sub.to_string()))?;

            // Subscribe to orderbook depth for this symbol
            let depth_sub = json!({
                "method": "sub.depth",
                "param": {
                    "symbol": symbol,
                    "limit": self.max_levels
                }
            });
            write_tx.send(Message::Text(depth_sub.to_string()))?;
        }

        info!("Subscribed to ticker, fair_price, and depth for {} symbols", self.symbols.len());

        // Spawn heartbeat task
        let write_tx_clone = write_tx.clone();
        tokio::spawn(async move {
            let mut heartbeat_interval = interval(Duration::from_secs(30));
            loop {
                heartbeat_interval.tick().await;
                let ping = json!({"method": "ping"});
                if write_tx_clone.send(Message::Text(ping.to_string())).is_err() {
                    break;
                }
            }
        });

        // Read messages
        let mut read = read;
        while let Some(msg) = read.next().await {
            match msg {
                Ok(Message::Text(text)) => {
                    if let Err(e) = self.handle_message(&text, event_tx) {
                        warn!("Failed to handle message: {:?}", e);
                    }
                }
                Ok(Message::Ping(_)) => {
                    // Handled automatically by tungstenite
                }
                Ok(Message::Pong(_)) => {
                    // Handled automatically by tungstenite
                }
                Ok(Message::Close(_)) => {
                    warn!("WebSocket closed by server");
                    break;
                }
                Err(e) => {
                    error!("WebSocket error: {:?}", e);
                    break;
                }
                _ => {}
            }
        }

        write_handle.abort();
        Ok(())
    }

    fn handle_message(&self, text: &str, event_tx: &mpsc::UnboundedSender<MarketEvent>) -> Result<()> {
        let value: Value = serde_json::from_str(text)?;

        // Check for pong
        if let Some(channel) = value.get("channel").and_then(|c| c.as_str()) {
            if channel == "pong" {
                return Ok(());
            }

            match channel {
                "push.ticker" => {
                    if let Some(data) = value.get("data") {
                        let ticker: TickerData = serde_json::from_value(data.clone())?;
                        self.handle_ticker(ticker, event_tx)?;
                    }
                }
                "push.fair_price" => {
                    if let Some(data) = value.get("data") {
                        let mark_price: MarkPriceData = serde_json::from_value(data.clone())?;
                        self.handle_mark_price(mark_price, event_tx)?;
                    }
                }
                "push.depth" => {
                    if let Some(symbol) = value.get("symbol").and_then(|s| s.as_str()) {
                        if let Some(data) = value.get("data") {
                            let mut orderbook: OrderbookData = serde_json::from_value(data.clone())?;
                            orderbook.symbol = Some(symbol.to_string());
                            self.handle_orderbook(orderbook, event_tx)?;
                        }
                    }
                }
                _ => {
                    // Ignore subscription confirmations (rs.sub.*) and other non-data channels
                }
            }
        }

        Ok(())
    }

    fn handle_ticker(&self, ticker: TickerData, event_tx: &mpsc::UnboundedSender<MarketEvent>) -> Result<()> {
        let last_price = ticker.last_price.parse::<f64>()?;
        let mark_price = ticker.fair_price.as_ref().and_then(|p| p.parse::<f64>().ok());
        let timestamp = DateTime::from_timestamp_millis(ticker.timestamp)
            .unwrap_or_else(Utc::now);

        let event = MarketEvent::TickerUpdate {
            symbol: ticker.symbol,
            last_price,
            mark_price,
            timestamp,
        };

        event_tx.send(event)?;
        Ok(())
    }

    fn handle_mark_price(&self, data: MarkPriceData, event_tx: &mpsc::UnboundedSender<MarketEvent>) -> Result<()> {
        let mark_price = data.fair_price.parse::<f64>()?;
        let timestamp = DateTime::from_timestamp_millis(data.timestamp)
            .unwrap_or_else(Utc::now);

        let event = MarketEvent::MarkPriceUpdate {
            symbol: data.symbol,
            mark_price,
            timestamp,
        };

        event_tx.send(event)?;
        Ok(())
    }

    fn handle_orderbook(&self, data: OrderbookData, event_tx: &mpsc::UnboundedSender<MarketEvent>) -> Result<()> {
        let symbol = data.symbol.clone().ok_or_else(|| anyhow::anyhow!("Missing symbol in orderbook"))?;
        let orderbook = ProcessedOrderbook::from_raw(&data, self.max_levels);

        let event = MarketEvent::OrderbookUpdate {
            symbol,
            orderbook,
        };

        event_tx.send(event)?;
        Ok(())
    }
}
