use crate::models::market_data::{Candle, SymbolData};
use anyhow::Result;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{error, info};

#[derive(Debug, Clone)]
struct RecordingSession {
    symbol: String,
    strategy_name: String,
    start_time: DateTime<Utc>,
    anomaly_ended: Option<DateTime<Utc>>,
    last_price_candles: Vec<Candle>,
    mark_price_candles: Vec<Candle>,
}

impl RecordingSession {
    fn new(symbol: String, strategy_name: String, pre_buffer_candles: (Vec<Candle>, Vec<Candle>)) -> Self {
        Self {
            symbol,
            strategy_name,
            start_time: Utc::now(),
            anomaly_ended: None,
            last_price_candles: pre_buffer_candles.0,
            mark_price_candles: pre_buffer_candles.1,
        }
    }

    fn add_candles(&mut self, candles: (Vec<Candle>, Vec<Candle>)) {
        self.last_price_candles.extend(candles.0);
        self.mark_price_candles.extend(candles.1);
    }
}

#[derive(Clone)]
pub struct CsvExporter {
    charts_dir: PathBuf,
    post_anomaly_recording_secs: i64,
    active_recordings: Arc<DashMap<String, RecordingSession>>,
    symbol_data: Arc<DashMap<String, SymbolData>>,
}

impl CsvExporter {
    pub fn new(
        charts_dir: &str,
        post_anomaly_recording_secs: i64,
        symbol_data: Arc<DashMap<String, SymbolData>>,
    ) -> Result<Self> {
        // Create charts directory if it doesn't exist
        fs::create_dir_all(charts_dir)?;

        Ok(Self {
            charts_dir: PathBuf::from(charts_dir),
            post_anomaly_recording_secs,
            active_recordings: Arc::new(DashMap::new()),
            symbol_data,
        })
    }

    pub fn start_recording(&self, symbol: &str, strategy_name: &str, pre_buffer_candles: (Vec<Candle>, Vec<Candle>)) {
        info!("[CsvExporter] start_recording() called for {} ({})", symbol, strategy_name);

        let recording_key = format!("{}_{}", symbol, strategy_name);

        // Check if already recording for this symbol+strategy combination
        if self.active_recordings.contains_key(&recording_key) {
            info!("[CsvExporter] Already recording for {} ({})", symbol, strategy_name);
            return;
        }

        info!(
            "[CsvExporter] Received {} last_price candles and {} mark_price candles as pre-buffer",
            pre_buffer_candles.0.len(), pre_buffer_candles.1.len()
        );

        info!("[CsvExporter] Creating recording session for {}", recording_key);

        let session = RecordingSession::new(
            symbol.to_string(),
            strategy_name.to_string(),
            pre_buffer_candles,
        );

        self.active_recordings.insert(recording_key.clone(), session);

        info!(
            "[CsvExporter] ✅ Recording session started for {} ({}) - Total active recordings: {}",
            symbol, strategy_name, self.active_recordings.len()
        );
    }

    pub fn update_recording(&self, symbol: &str) {
        // Update all active recordings for this symbol
        let recordings: Vec<String> = self
            .active_recordings
            .iter()
            .filter(|entry| entry.value().symbol == symbol && entry.value().anomaly_ended.is_none())
            .map(|entry| entry.key().clone())
            .collect();

        for recording_key in recordings {
            if let Some(data) = self.symbol_data.get(symbol) {
                // Get the latest completed candles
                let new_candles = data.candle_buffer.get_all_completed_candles();

                if let Some(mut session) = self.active_recordings.get_mut(&recording_key) {
                    session.add_candles(new_candles);
                }
            }
        }
    }

    pub fn mark_anomaly_ended(&self, symbol: &str, strategy_name: &str) {
        info!("[CsvExporter] mark_anomaly_ended() called for {} ({})", symbol, strategy_name);

        let recording_key = format!("{}_{}", symbol, strategy_name);

        if let Some(mut session) = self.active_recordings.get_mut(&recording_key) {
            session.anomaly_ended = Some(Utc::now());
            info!(
                "[CsvExporter] ✅ Marked anomaly ended for {} ({}), will continue recording for {} more seconds",
                symbol, strategy_name, self.post_anomaly_recording_secs
            );
        } else {
            info!("[CsvExporter] WARNING: No active recording found for {}", recording_key);
            return;
        }

        // Spawn background task to finalize after delay
        info!("[CsvExporter] Spawning background task to finalize recording after {} seconds", self.post_anomaly_recording_secs);

        let exporter = self.clone();
        let symbol_owned = symbol.to_string();
        let strategy_owned = strategy_name.to_string();
        let post_secs = self.post_anomaly_recording_secs;

        tokio::spawn(async move {
            info!("[CsvExporter] Background task started - waiting {} seconds before finalizing {}", post_secs, symbol_owned);
            sleep(Duration::from_secs(post_secs as u64)).await;
            info!("[CsvExporter] Wait complete - now finalizing recording for {}", symbol_owned);

            if let Err(e) = exporter.finalize_recording(&symbol_owned, &strategy_owned).await {
                error!("[CsvExporter] Failed to finalize recording for {} ({}): {}", symbol_owned, strategy_owned, e);
            } else {
                info!("[CsvExporter] Successfully finalized recording for {} ({})", symbol_owned, strategy_owned);
            }
        });

        info!("[CsvExporter] Background task spawned for {} ({})", symbol, strategy_name);
    }

    async fn finalize_recording(&self, symbol: &str, strategy_name: &str) -> Result<()> {
        info!("[CsvExporter] finalize_recording() called for {} ({})", symbol, strategy_name);

        let recording_key = format!("{}_{}", symbol, strategy_name);

        // Get the final candles from the buffer
        info!("[CsvExporter] Getting final candles from buffer...");
        if let Some(data) = self.symbol_data.get(symbol) {
            let final_candles = data.candle_buffer.get_all_completed_candles();
            info!(
                "[CsvExporter] Retrieved {} final last_price candles and {} mark_price candles",
                final_candles.0.len(), final_candles.1.len()
            );

            if let Some(mut session) = self.active_recordings.get_mut(&recording_key) {
                let before_count = session.last_price_candles.len();
                session.add_candles(final_candles);
                info!(
                    "[CsvExporter] Added final candles - session now has {} candles (was {})",
                    session.last_price_candles.len(), before_count
                );
            } else {
                info!("[CsvExporter] WARNING: Could not find recording session {}", recording_key);
            }
        } else {
            info!("[CsvExporter] WARNING: Could not find symbol data for {}", symbol);
        }

        // Remove the session and write CSV files
        info!("[CsvExporter] Removing recording session and writing CSV files...");
        if let Some((_, session)) = self.active_recordings.remove(&recording_key) {
            info!(
                "[CsvExporter] Writing CSV files with {} last_price candles and {} mark_price candles",
                session.last_price_candles.len(),
                session.mark_price_candles.len()
            );

            self.write_csv_files(&session)?;

            info!(
                "[CsvExporter] ✅ Finalized recording for {} ({}) - wrote {} candles to CSV files",
                symbol,
                strategy_name,
                session.last_price_candles.len()
            );
        } else {
            info!("[CsvExporter] WARNING: No recording session found to remove for {}", recording_key);
        }

        Ok(())
    }

    fn write_csv_files(&self, session: &RecordingSession) -> Result<()> {
        info!("[CsvExporter] write_csv_files() called for {} ({})", session.symbol, session.strategy_name);

        // Generate filename with datetime
        let datetime_str = session.start_time.format("%Y%m%d_%H%M%S").to_string();
        let last_price_filename = format!(
            "{}_{}_{}_{}.csv",
            session.symbol, session.strategy_name, datetime_str, "lastprice"
        );
        let mark_price_filename = format!(
            "{}_{}_{}_{}.csv",
            session.symbol, session.strategy_name, datetime_str, "fairprice"
        );

        info!("[CsvExporter] Generated filenames: {} and {}", last_price_filename, mark_price_filename);

        // Write last_price CSV
        let last_price_path = self.charts_dir.join(&last_price_filename);
        info!("[CsvExporter] Writing last_price CSV to: {}", last_price_path.display());
        self.write_candles_to_csv(&last_price_path, &session.last_price_candles)?;
        info!("[CsvExporter] ✅ Successfully wrote last_price CSV");

        // Write mark_price (fair_price) CSV
        let mark_price_path = self.charts_dir.join(&mark_price_filename);
        info!("[CsvExporter] Writing mark_price CSV to: {}", mark_price_path.display());
        self.write_candles_to_csv(&mark_price_path, &session.mark_price_candles)?;
        info!("[CsvExporter] ✅ Successfully wrote mark_price CSV");

        info!(
            "[CsvExporter] ✅✅ Wrote both CSV files for {} ({}):\n  - {}\n  - {}",
            session.symbol,
            session.strategy_name,
            last_price_path.display(),
            mark_price_path.display()
        );

        Ok(())
    }

    fn write_candles_to_csv(&self, path: &PathBuf, candles: &[Candle]) -> Result<()> {
        info!("[CsvExporter] write_candles_to_csv() - Writing {} candles to {}", candles.len(), path.display());

        let mut wtr = csv::Writer::from_path(path)?;
        info!("[CsvExporter] CSV writer created successfully");

        // Write header
        wtr.write_record(&["timestamp_ms", "open", "high", "low", "close", "volume"])?;
        info!("[CsvExporter] CSV header written");

        // Write candle data
        for (i, candle) in candles.iter().enumerate() {
            wtr.write_record(&[
                candle.timestamp_ms.to_string(),
                candle.open.to_string(),
                candle.high.to_string(),
                candle.low.to_string(),
                candle.close.to_string(),
                candle.volume.to_string(),
            ])?;

            if i < 3 || i == candles.len() - 1 {
                info!(
                    "[CsvExporter] Row {}: ts={}, o={:.4}, h={:.4}, l={:.4}, c={:.4}",
                    i, candle.timestamp_ms, candle.open, candle.high, candle.low, candle.close
                );
            }
        }

        wtr.flush()?;
        info!("[CsvExporter] ✅ CSV file flushed and closed successfully");
        Ok(())
    }

    pub fn is_recording(&self, symbol: &str, strategy_name: &str) -> bool {
        let recording_key = format!("{}_{}", symbol, strategy_name);
        self.active_recordings.contains_key(&recording_key)
    }
}
