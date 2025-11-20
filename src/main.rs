mod api;
mod config;
mod detection;
mod models;
mod utils;

use crate::api::{MexcRestClient, MexcWebSocketClient};
use crate::config::Config;
use crate::detection::{Strategy1, Strategy2, Strategy3, Strategy4};
use crate::models::{MarketEvent, SymbolData};
use crate::utils::EpisodeLogger;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter("mexc_sniper=info,warn,error")
        .init();

    info!("Starting MEXC Futures Pump Anomaly Detector");

    // Load environment variables
    dotenv::dotenv().ok();

    // Load configuration
    let config = Config::load("config.toml")?;
    info!("Configuration loaded successfully");

    // Initialize REST client and fetch symbols
    let rest_client = MexcRestClient::new(config.api.base_rest_url.clone());
    info!("Fetching contract list from exchange...");

    let all_symbols = rest_client.get_all_contracts().await?;
    info!("Found {} active contracts", all_symbols.len());

    // Determine which symbols to monitor
    let symbols_to_monitor = if config.general.symbols.is_empty() {
        all_symbols
    } else {
        config.general.symbols.clone()
    };

    info!("Monitoring {} symbols", symbols_to_monitor.len());

    // Initialize shared symbol data storage
    let symbol_data: Arc<DashMap<String, SymbolData>> = Arc::new(DashMap::new());

    for symbol in &symbols_to_monitor {
        symbol_data.insert(symbol.clone(), SymbolData::new(symbol.clone()));
    }

    // Initialize episode loggers
    let logger1 = Arc::new(EpisodeLogger::new(&config.general.log_dir, "strategy1")?);
    let logger2 = Arc::new(EpisodeLogger::new(&config.general.log_dir, "strategy2")?);
    let logger3 = Arc::new(EpisodeLogger::new(&config.general.log_dir, "strategy3")?);
    let logger4 = Arc::new(EpisodeLogger::new(&config.general.log_dir, "strategy4")?);

    info!("Episode loggers initialized");

    // Initialize strategies
    let mut strategy1 = Strategy1::new(
        config.strategy1.clone(),
        config.cooldowns.per_symbol_seconds,
        logger1,
    );

    let mut strategy2 = Strategy2::new(
        config.strategy2.clone(),
        config.cooldowns.per_symbol_seconds,
        logger2,
    );

    let mut strategy3 = Strategy3::new(
        config.strategy3.clone(),
        config.cooldowns.per_symbol_seconds,
        logger3,
    );

    let mut strategy4 = Strategy4::new(
        config.strategy4.clone(),
        config.orderbook.clone(),
        config.cooldowns.per_symbol_seconds,
        logger4,
    );

    info!("Detection strategies initialized");

    // Create WebSocket client
    let ws_client = MexcWebSocketClient::new(
        config.api.base_ws_url.clone(),
        symbols_to_monitor.clone(),
        config.orderbook.max_levels,
    );

    // Create channel for market events
    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<MarketEvent>();

    // Spawn WebSocket task
    let ws_handle = tokio::spawn(async move {
        if let Err(e) = ws_client.run(event_tx).await {
            error!("WebSocket task failed: {:?}", e);
        }
    });

    info!("WebSocket connection established");
    info!("System running - monitoring for pump anomalies...");

    // Create periodic status logger
    let symbol_data_clone = symbol_data.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60));
        loop {
            interval.tick().await;
            let symbols_with_data: Vec<_> = symbol_data_clone
                .iter()
                .filter(|entry| entry.value().current_last_price.is_some())
                .map(|entry| entry.key().clone())
                .collect();

            info!(
                "Status: Monitoring {} symbols | Active data streams: {} | Uptime: OK",
                symbol_data_clone.len(),
                symbols_with_data.len()
            );

            // Log a few price samples
            if !symbols_with_data.is_empty() {
                for symbol in symbols_with_data.iter().take(3) {
                    if let Some(data) = symbol_data_clone.get(symbol) {
                        if let (Some(last), Some(mark)) = (data.current_last_price, data.current_mark_price) {
                            let ratio = last / mark;
                            info!(
                                "  {} | Last: {:.4} | Mark: {:.4} | Ratio: {:.6}",
                                symbol, last, mark, ratio
                            );
                        }
                    }
                }
            }
        }
    });

    // Main event loop
    loop {
        tokio::select! {
            Some(event) = event_rx.recv() => {
                handle_market_event(
                    event,
                    &symbol_data,
                    &mut strategy1,
                    &mut strategy2,
                    &mut strategy3,
                    &mut strategy4,
                );
            }
            _ = tokio::signal::ctrl_c() => {
                info!("Received shutdown signal");
                break;
            }
        }
    }

    info!("Shutting down gracefully...");
    ws_handle.abort();

    Ok(())
}

fn handle_market_event(
    event: MarketEvent,
    symbol_data: &Arc<DashMap<String, SymbolData>>,
    strategy1: &mut Strategy1,
    strategy2: &mut Strategy2,
    strategy3: &mut Strategy3,
    strategy4: &mut Strategy4,
) {
    match event {
        MarketEvent::TickerUpdate {
            symbol,
            last_price,
            mark_price,
            timestamp,
        } => {
            if let Some(mut data) = symbol_data.get_mut(&symbol) {
                data.update_last_price(last_price, timestamp);

                if let Some(mark) = mark_price {
                    data.update_mark_price(mark, timestamp);
                }

                // Run all strategies
                strategy1.check(&data);
                strategy2.check(&data);
                strategy3.check(&data);
                strategy4.check(&data);
            }
        }
        MarketEvent::MarkPriceUpdate {
            symbol,
            mark_price,
            timestamp,
        } => {
            if let Some(mut data) = symbol_data.get_mut(&symbol) {
                data.update_mark_price(mark_price, timestamp);

                // Run all strategies
                strategy1.check(&data);
                strategy2.check(&data);
                strategy3.check(&data);
                strategy4.check(&data);
            }
        }
        MarketEvent::OrderbookUpdate { symbol, orderbook } => {
            if let Some(mut data) = symbol_data.get_mut(&symbol) {
                data.update_orderbook(orderbook);

                // Run strategy 4 which uses orderbook data
                strategy4.check(&data);
            }
        }
    }
}
