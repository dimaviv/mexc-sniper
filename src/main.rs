mod api;
mod config;
mod detection;
mod export;
mod models;
mod utils;

use crate::api::{MexcRestClient, MexcWebSocketClient};
use crate::config::Config;
use crate::detection::{Strategy1, Strategy2, Strategy3, Strategy4, Strategy5};
use crate::export::CsvExporter;
use crate::models::{MarketEvent, SymbolData};
use crate::utils::EpisodeLogger;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use rand::{seq::IteratorRandom, SeedableRng};
use tracing::{debug, error, info};
use tracing_subscriber;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with debug level for more visibility
    tracing_subscriber::fmt()
        .with_env_filter("mexc_sniper=debug")
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
    let logger5 = Arc::new(EpisodeLogger::new(&config.general.log_dir, "strategy5")?);

    info!("Episode loggers initialized");

    // Initialize CSV exporter if enabled
    let csv_exporter = if config.csv_export.enabled {
        let exporter = CsvExporter::new(
            &config.csv_export.charts_dir,
            config.csv_export.post_anomaly_recording_secs,
            symbol_data.clone(),
        )?;
        info!("CSV exporter initialized - charts will be saved to: {}", config.csv_export.charts_dir);
        Some(Arc::new(exporter))
    } else {
        info!("CSV export is disabled");
        None
    };

    let pre_buffer_secs = config.csv_export.pre_anomaly_buffer_secs;

    // Initialize strategies
    let mut strategy1 = Strategy1::new(
        config.strategy1.clone(),
        config.cooldowns.per_symbol_seconds,
        logger1,
        csv_exporter.clone(),
        pre_buffer_secs,
    );

    let mut strategy2 = Strategy2::new(
        config.strategy2.clone(),
        config.cooldowns.per_symbol_seconds,
        logger2,
        csv_exporter.clone(),
        pre_buffer_secs,
    );

    let mut strategy3 = Strategy3::new(
        config.strategy3.clone(),
        config.cooldowns.per_symbol_seconds,
        logger3,
        csv_exporter.clone(),
        pre_buffer_secs,
    );

    let mut strategy4 = Strategy4::new(
        config.strategy4.clone(),
        config.orderbook.clone(),
        config.cooldowns.per_symbol_seconds,
        logger4,
        csv_exporter.clone(),
        pre_buffer_secs,
    );

    let mut strategy5 = Strategy5::new(
        config.strategy5.clone(),
        config.strategy1.clone(),
        config.strategy2.clone(),
        config.strategy3.clone(),
        config.strategy4.clone(),
        config.orderbook.clone(),
        config.cooldowns.per_symbol_seconds,
        logger5,
        csv_exporter.clone(),
        pre_buffer_secs,
    );

    info!("Detection strategies initialized (including Strategy5: Ultra-Strict)");

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

    // Create periodic detailed trace logger (every 10 seconds, random symbol)
    let symbol_data_for_trace = symbol_data.clone();
    let config_for_trace = config.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(10));
        let mut rng = rand::rngs::SmallRng::from_os_rng();

        loop {
            interval.tick().await;

            // Get symbols that have both prices available
            let symbols_with_data: Vec<_> = symbol_data_for_trace
                .iter()
                .filter(|entry| {
                    entry.value().current_last_price.is_some()
                        && entry.value().current_mark_price.is_some()
                })
                .map(|entry| entry.key().clone())
                .collect();

            if symbols_with_data.is_empty() {
                continue;
            }

            // Pick a random symbol
            let random_symbol = symbols_with_data.iter().choose(&mut rng);

            if let Some(symbol) = random_symbol {
                if let Some(data) = symbol_data_for_trace.get(symbol) {
                    let last_price = data.current_last_price.unwrap();
                    let mark_price = data.current_mark_price.unwrap();
                    let ratio = last_price / mark_price;
                    let abs_diff = last_price - mark_price;

                    // Strategy thresholds from config
                    let s1 = &config_for_trace.strategy1;
                    let s2 = &config_for_trace.strategy2;
                    let s3 = &config_for_trace.strategy3;
                    let s4 = &config_for_trace.strategy4;

                    // Check strategy conditions
                    let s1_ratio_ok = ratio >= s1.spread_ratio_min;
                    let s1_diff_ok = abs_diff >= s1.min_abs_diff;
                    let s1_price_ok = last_price >= s1.min_price;
                    let s1_triggered = s1.enabled && s1_ratio_ok && s1_diff_ok && s1_price_ok;

                    let s2_ratio_ok = ratio >= s2.spread_ratio_min;
                    let s2_price_ok = last_price >= s2.min_price;

                    let s3_ratio_ok = ratio >= s3.spread_ratio_min;
                    let s3_price_ok = last_price >= s3.min_price;

                    let s4_ratio_ok = ratio >= s4.spread_ratio_min;
                    let s4_diff_ok = abs_diff >= s4.min_abs_diff;
                    let s4_price_ok = last_price >= s4.min_price;

                    // Check orderbook data availability
                    let has_orderbook = data.orderbook.is_some();

                    info!("══════════════════════════════════════════════════════════════");
                    info!("[TRACE] Random Symbol Check: {}", symbol);
                    info!("├─ Last Price:    {:.6}", last_price);
                    info!("├─ Mark Price:    {:.6}", mark_price);
                    info!("├─ Ratio:         {:.6} (last/mark)", ratio);
                    info!("├─ Abs Diff:      {:.6} (last - mark)", abs_diff);
                    info!("├─ Orderbook:     {}", if has_orderbook { "Available" } else { "Not available" });
                    info!("├─ Strategy1 [{}]:", if s1.enabled { "ON" } else { "OFF" });
                    info!("│  ├─ Ratio >= {:.4}?  {} (actual: {:.6})",
                        s1.spread_ratio_min,
                        if s1_ratio_ok { "YES" } else { "NO" },
                        ratio
                    );
                    info!("│  ├─ Diff >= {:.4}?   {} (actual: {:.6})",
                        s1.min_abs_diff,
                        if s1_diff_ok { "YES" } else { "NO" },
                        abs_diff
                    );
                    info!("│  ├─ Price >= {:.4}? {} (actual: {:.6})",
                        s1.min_price,
                        if s1_price_ok { "YES" } else { "NO" },
                        last_price
                    );
                    info!("│  └─ TRIGGERED:    {}", if s1_triggered { "YES" } else { "NO" });
                    info!("├─ Strategy2 [{}]: Ratio {} | Price {}",
                        if s2.enabled { "ON" } else { "OFF" },
                        if s2_ratio_ok { "OK" } else { "NO" },
                        if s2_price_ok { "OK" } else { "NO" }
                    );
                    info!("├─ Strategy3 [{}]: Ratio {} | Price {}",
                        if s3.enabled { "ON" } else { "OFF" },
                        if s3_ratio_ok { "OK" } else { "NO" },
                        if s3_price_ok { "OK" } else { "NO" }
                    );
                    info!("├─ Strategy4 [{}]: Ratio {} | Diff {} | Price {}",
                        if s4.enabled { "ON" } else { "OFF" },
                        if s4_ratio_ok { "OK" } else { "NO" },
                        if s4_diff_ok { "OK" } else { "NO" },
                        if s4_price_ok { "OK" } else { "NO" }
                    );
                    info!("└─ Strategy5 [{}]: Combines all above conditions",
                        if config_for_trace.strategy5.enabled { "ON" } else { "OFF" }
                    );
                    info!("══════════════════════════════════════════════════════════════");
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
                    &mut strategy5,
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
    strategy5: &mut Strategy5,
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
                strategy5.check(&data);
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
                strategy5.check(&data);
            }
        }
        MarketEvent::OrderbookUpdate { symbol, orderbook } => {
            if let Some(mut data) = symbol_data.get_mut(&symbol) {
                data.update_orderbook(orderbook);

                // Run strategies that use orderbook data
                strategy4.check(&data);
                strategy5.check(&data);
            }
        }
    }
}
