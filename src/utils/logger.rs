use chrono::{DateTime, Utc};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct EpisodeLogger {
    file_path: PathBuf,
    file: Mutex<std::fs::File>,
}

impl EpisodeLogger {
    pub fn new(log_dir: &str, strategy_name: &str) -> anyhow::Result<Self> {
        fs::create_dir_all(log_dir)?;

        let file_path = PathBuf::from(log_dir).join(format!("{}_episodes.log", strategy_name));

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file_path)?;

        Ok(Self {
            file_path,
            file: Mutex::new(file),
        })
    }

    pub fn log_episode(
        &self,
        symbol: &str,
        start_time: DateTime<Utc>,
        end_time: DateTime<Utc>,
        peak_ratio: f64,
        peak_last: f64,
        peak_mark: f64,
    ) -> anyhow::Result<()> {
        let duration = end_time.signed_duration_since(start_time);
        let duration_str = format!("{}s", duration.num_seconds());

        let log_line = format!(
            "{} | {} | START={} | END={} | DURATION={} | PEAK_RATIO={:.4} | PEAK_LAST={:.8} | PEAK_MARK={:.8}\n",
            end_time.format("%Y-%m-%dT%H:%M:%SZ"),
            symbol,
            start_time.format("%H:%M:%S"),
            end_time.format("%H:%M:%S"),
            duration_str,
            peak_ratio,
            peak_last,
            peak_mark
        );

        let mut file = self.file.lock().unwrap();
        file.write_all(log_line.as_bytes())?;
        file.flush()?;

        Ok(())
    }
}
