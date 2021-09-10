# telemetry-parser
A tool to parse real-time metadata embedded in video files or telemetry from other sources.

Work in progress, the code is already working but I plan to add much more input and output formats.

# Supported formats:
- [x] Sony (RX0 II, a7s III, RX100 VII, ZV1, a7c, a7r IV, a6600, a9 II, a1, FX3, ZV-E10)
- [x] GoPro (All models with gyro metadata, starting with HERO 5)
- [x] Insta360 (OneR, SMO 4k, GO2)
- [ ] TODO Betaflight blackbox (CSV and Binary)
- [ ] TODO Runcam CSV
- [ ] TODO DJI flight logs (*.dat, *.txt)

# Example usage
Produce Betaflight blackbox CSV with gyroscope and accelerometer from the input file
```
gyro2bb file.mp4
```
Dump all metadata found in the source file.
```
gyro2bb --dump file.mp4
```

# Building
1. Get latest stable Rust language from: https://rustup.rs/
2. Clone the repo: `git clone https://github.com/AdrianEddy/telemetry-parser.git`
3. Build the binary: `cd bin/gyro2bb ; cargo build --release`
4. Resulting file will be in `target/release/` directory
