# telemetry-parser-py
`telemetry-parser` python module

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
1. Setup virtual env: `python -m venv .env ; source .env/bin/activate ; pip install maturin`
2. Install maturin: `pip install maturin`
2. Build python wheels: `maturin build --release`
3. Resulting wheels will be in `target/wheels/` directory
4. Install using pip: `pip install telemetry_parser_py-0.1.0-cp39-none-win_amd64.wh`