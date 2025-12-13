# telemetry-parser-py
Library to parse real-time metadata embedded in video files or telemetry from other sources.

# Supported formats:
- [x] GoPro (HERO 5 and later)
- [x] Sony (a1, a7c, a7r V, a7 IV, a7s III, a9 II, a9 III, FX3, FX6, FX9, RX0 II, RX100 VII, ZV1, ZV-E10, ZV-E10 II, ZV-E1, a6700)
- [x] Insta360 (OneR, OneRS, SMO 4k, Go, GO2, GO3, GO3S, GOUltra, Caddx Peanut, Ace, Ace Pro)
- [x] DJI (Avata, Avata 2, O3/O4 Air Unit, Action 2/4/5/6/Nano, Neo, Neo2)
- [x] XTRA (Edge, Edge Pro)
- [x] Blackmagic RAW (*.braw)
- [x] RED RAW (V-Raptor, KOMODO) (*.r3d)
- [x] Canon (C50, C80, C400, R6 Mk3, R5 Mk2) (*.mp4, *.mov, *.mxf, *.crm)
- [x] Freefly (Ember)
- [x] Betaflight blackbox (*.bfl, *.bbl, *.csv)
- [x] ArduPilot logs (*.bin, *.log)
- [x] Gyroflow [.gcsv log](https://docs.gyroflow.xyz/app/technical-details/gcsv-format)
- [x] iOS apps: [`Sensor Logger`](https://apps.apple.com/us/app/sensor-logger/id1531582925), [`G-Field Recorder`](https://apps.apple.com/at/app/g-field-recorder/id1154585693), [`Gyro`](https://apps.apple.com/us/app/gyro-record-device-motion-data/id1161532981)
- [x] Android apps: [`Sensor Logger`](https://play.google.com/store/apps/details?id=com.kelvin.sensorapp&hl=de_AT&gl=US), [`Sensor Record`](https://play.google.com/store/apps/details?id=de.martingolpashin.sensor_record), [`OpenCamera Sensors`](https://github.com/MobileRoboticsSkoltech/OpenCamera-Sensors), [`MotionCam Pro`](https://play.google.com/store/apps/details?id=com.motioncam.pro)
- [x] Runcam CSV (Runcam 5 Orange, iFlight GOCam GR, Runcam Thumb, Mobius Maxi 4K)
- [x] Hawkeye Firefly X Lite CSV
- [x] XTU (S2Pro, S3Pro)
- [x] WitMotion (WT901SDCL binary and *.txt)
- [x] Vuze (VuzeXR)
- [x] KanDao (Obisidian Pro, Qoocam EGO)
- [x] [CAMM format](https://developers.google.com/streetview/publish/camm-spec)
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