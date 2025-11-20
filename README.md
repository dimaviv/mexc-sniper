# MEXC Futures Pump Anomaly Detector

A production-ready Rust application that monitors MEXC futures trading pairs in real-time to detect "pump anomalies" where the last traded price significantly exceeds the mark/fair price.

## Features

- **Real-time monitoring** via WebSocket connections to MEXC futures API
- **Four independent detection strategies** with configurable thresholds
- **Episode tracking** - logs complete anomaly episodes from start to finish
- **Configurable** - All parameters adjustable via TOML config file
- **Robust error handling** - Automatic WebSocket reconnection with exponential backoff
- **Production-ready** - Comprehensive logging, graceful shutdown, concurrent data processing

## Detection Strategies

### Strategy 1: Simple Spread Ratio
Detects when `last_price / mark_price >= threshold` with configurable minimum absolute difference.

### Strategy 2: Spread + Recent Spike
Combines spread ratio check with spike detection - compares current price to historical price N seconds ago.

### Strategy 3: Spread + Baseline Stability
Detects pumps relative to a rolling baseline window while ensuring mark price remains stable.

### Strategy 4: Spread + Thick Orderbook
Only triggers on thick orderbooks with tight spreads and significant depth near the mid-price.

## Installation

### Prerequisites
- Rust 1.70+ (install from [rustup.rs](https://rustup.rs/))
- Internet connection for MEXC API access

### Setup

1. Clone or download this repository

2. Copy the example environment file:
   ```bash
   cp .env.example .env
   ```

3. (Optional) Edit `.env` with your MEXC API credentials:
   ```
   MEXC_API_KEY=your_api_key_here
   MEXC_API_SECRET=your_api_secret_here
   ```
   Note: Public market data endpoints don't require authentication.

4. Review and adjust `config.toml` to your preferences:
   - Symbol selection (empty = monitor all futures pairs)
   - Strategy thresholds and enable/disable flags
   - Cooldown periods
   - Orderbook parameters

5. Build the project:
   ```bash
   cargo build --release
   ```

## Usage

### Running the Application

```bash
cargo run --release
```

Or run the compiled binary directly:
```bash
./target/release/mexc-sniper
```

### Output

The application will:
1. Connect to MEXC API and fetch all available futures contracts
2. Subscribe to real-time market data via WebSocket
3. Monitor all configured symbols continuously
4. Log detected anomaly episodes to separate files per strategy

### Log Files

Episodes are logged to `logs/` directory (created automatically):
- `logs/strategy1_episodes.log`
- `logs/strategy2_episodes.log`
- `logs/strategy3_episodes.log`
- `logs/strategy4_episodes.log`

#### Log Format
```
2025-11-20T12:34:56Z | BTC_USDT | START=12:34:50 | END=12:34:56 | DURATION=6s | PEAK_RATIO=1.85 | PEAK_LAST=43500.0 | PEAK_MARK=23513.5
```

### Graceful Shutdown

Press `Ctrl+C` to stop the application. It will:
- Close WebSocket connections
- Flush all logs
- Exit cleanly

## Configuration

### config.toml Structure

```toml
[api]
base_rest_url = "https://contract.mexc.com"
base_ws_url = "wss://contract.mexc.com/ws"

[general]
symbols = []  # Empty = monitor all, or specify: ["BTC_USDT", "ETH_USDT"]
log_dir = "logs"
poll_interval_ms = 500

[cooldowns]
per_symbol_seconds = 60  # Minimum time between episodes per symbol

[orderbook]
max_levels = 20
depth_band_pct = 0.005  # ±0.5% around mid-price
min_thick_depth_usdt = 10000.0
max_spread_pct = 0.003

[strategy1]
enabled = true
spread_ratio_min = 1.5
min_abs_diff = 0.0001
min_price = 0.01

[strategy2]
enabled = true
spread_ratio_min = 1.3
spike_lookback_secs = 5
spike_ratio_min = 1.2
min_price = 0.01

[strategy3]
enabled = true
spread_ratio_min = 1.2
baseline_window_secs = 60
pump_vs_baseline_min = 1.5
mark_stability_max = 0.05
min_price = 0.01

[strategy4]
enabled = true
spread_ratio_min = 1.5
min_abs_diff = 0.0001
min_price = 0.01
```

## Architecture

```
src/
├── main.rs              - Application entry point and event loop
├── config.rs            - Configuration parsing and structures
├── api/
│   ├── rest.rs          - REST API client for exchange info
│   └── websocket.rs     - WebSocket client with auto-reconnect
├── models/
│   ├── market_data.rs   - Market data structures and processing
│   └── events.rs        - Internal event types
├── detection/
│   ├── episode.rs       - Episode tracking logic
│   ├── strategy1.rs     - Strategy implementations
│   ├── strategy2.rs
│   ├── strategy3.rs
│   └── strategy4.rs
└── utils/
    └── logger.rs        - Episode logging to files
```

## How It Works

1. **Initialization**: Fetches all active futures contracts from MEXC REST API
2. **Connection**: Establishes WebSocket connection and subscribes to:
   - Ticker updates (last price, mark price)
   - Fair price updates
   - Orderbook depth updates
3. **Data Processing**: Maintains real-time state for each symbol:
   - Current last price and mark price
   - Historical price data (ring buffer)
   - Orderbook snapshot
4. **Detection**: On each update, runs all enabled strategies
5. **Episode Tracking**:
   - Starts episode when conditions first met
   - Updates peak values while conditions persist
   - Ends episode and logs when conditions no longer met
   - Applies cooldown period before next episode

## Performance

- **Concurrent processing** using Tokio async runtime
- **Lock-free data structures** (DashMap) for symbol data
- **Efficient WebSocket handling** with minimal overhead
- Can monitor hundreds of symbols simultaneously

## Data Collection Only

**IMPORTANT**: This application is for monitoring and data collection purposes ONLY. It does NOT:
- Open or manage trades
- Execute any orders
- Interact with account funds
- Require private API credentials for core functionality

## Troubleshooting

### Connection Issues
- Check internet connectivity
- Verify MEXC API endpoints are accessible
- Review firewall/proxy settings

### No Data Received
- Ensure symbols in config exist on MEXC futures
- Check WebSocket subscription confirmations in logs
- Verify market is active (not maintenance period)

### High CPU/Memory Usage
- Reduce number of monitored symbols
- Increase cooldown periods
- Adjust orderbook max_levels

## Development

### Building for Production
```bash
cargo build --release
```

### Running Tests
```bash
cargo test
```

### Debug Logging
Set environment variable for verbose logging:
```bash
RUST_LOG=debug cargo run
```

## License

This is a custom application for personal use.

## Disclaimer

This software is provided for educational and research purposes. Use at your own risk. The authors are not responsible for any financial losses or trading decisions made based on this tool's output.

Trading cryptocurrencies carries significant risk. Always do your own research and never trade more than you can afford to lose.
