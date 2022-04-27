# telemetry-parser
A tool to parse real-time metadata embedded in video files or telemetry from other sources.

Work in progress, the code is already working but I plan to add much more input and output formats.

# Supported formats:
- [x] Sony (a1, a7c, a7r IV, a7 IV, a7s III, a9 II, FX3, FX6, FX9, RX0 II, RX100 VII, ZV1, ZV-E10)
- [x] GoPro (All models with gyro metadata, starting with HERO 5)
- [x] Insta360 (OneR, OneRS, SMO 4k, Go, GO2)
- [x] Betaflight blackbox (CSV and binary)
- [x] Runcam CSV (Runcam 5 Orange, iFlight GOCam GR, Runcam Thumb, Mobius Maxi 4K)
- [x] Hawkeye Firefly X Lite CSV
- [x] WitMotion (WT901SDCL binary and *.txt)
- [x] iOS apps: `Sensor Logger`, [`G-Field Recorder`](https://apps.apple.com/at/app/g-field-recorder/id1154585693), [`Gyro`](https://apps.apple.com/us/app/gyro-record-device-motion-data/id1161532981)
- [x] Android apps: [`Sensor Logger`](https://play.google.com/store/apps/details?id=com.kelvin.sensorapp&hl=de_AT&gl=US), [`Sensor Record`](https://play.google.com/store/apps/details?id=de.martingolpashin.sensor_record)
- [x] Gyroflow [.gcsv log](https://docs.gyroflow.xyz/logging/gcsv/)
- [x] ArduPilot (*.log)
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


# Python module
Python module is available on [PyPI](https://pypi.org/project/telemetry-parser/).
Details in [bin/python-module](https://github.com/AdrianEddy/telemetry-parser/tree/master/bin/python-module)


# Building
1. Get latest stable Rust language from: https://rustup.rs/
2. Clone the repo: `git clone https://github.com/AdrianEddy/telemetry-parser.git`
3. Build the binary: `cd bin/gyro2bb ; cargo build --release`
4. Resulting file will be in `target/release/` directory
