# telemetry-parser-py
Library to parse real-time metadata embedded in video files or telemetry from other sources.

Work in progress, the code is already working but I plan to add much more input and output formats.

# Supported formats:
- [x] Sony (RX0 II, a7s III, RX100 VII, ZV1, a7c, a7r IV, a6600, a9 II, a1, FX3, ZV-E10, FX6)
- [x] GoPro (All models with gyro metadata, starting with HERO 5)
- [x] Insta360 (OneR, SMO 4k, GO2)
- [x] Betaflight blackbox (CSV and Binary)
- [x] Runcam CSV (Runcam 5 Orange, iFlight GOCam GR)
- [ ] TODO DJI flight logs (*.dat, *.txt)

# Example usage:
```python
import telemetry_parser

tp = telemetry_parser.Parser('sample.mp4')
print('Camera: ', tp.camera)
print('Model: ', tp.model)

# return all telemetry as an array of dicts
print('Telemetry', tp.telemetry()) 

# format the values with units etc
print('Telemetry formatted', tp.telemetry(human_readable = True)) 

# return only gyro and accel with timestamps, normalized to a single orientation and scaled to deg/s and m/s2
print('Normalized IMU', tp.normalized_imu()) 
```

# Building
1. Setup virtual env: `python -m venv .env ; source .env/bin/activate`
2. Install maturin: `pip install maturin`
2. Build python wheels: `maturin build --release`
3. Resulting wheels will be in `target/wheels/` directory
4. Install using pip: `pip install telemetry_parser_py-0.1.0-cp39-none-win_amd64.whl`