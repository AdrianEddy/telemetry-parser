// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use byteorder::{ ReadBytesExt, BigEndian };

fn b1(v: u8) -> u32 { (v & 0b1) as u32 }
fn b4(v: u8) -> u32 { (v & 0b1111) as u32 }
fn b6(v: u8) -> u32 { (v & 0b111111) as u32 }
fn b7(v: u8) -> u32 { (v & 0b1111111) as u32 }
fn ri16(d: &mut &[u8]) -> i16 { d.read_i16::<BigEndian>().unwrap() }
fn ru16(d: &mut &[u8]) -> u16 { d.read_u16::<BigEndian>().unwrap() }

fn parse_kd(d: &mut &[u8]) -> Option<serde_json::Value> {
    if d.len() >= 36 {
        let mut json = serde_json::json!({
            "RecordType": "rt.temporal.lens.general",
            "FocusDistance": (b6(d[0]) << 18) | (b6(d[1]) << 12) | (b6(d[2]) << 6) | b6(d[3]),
            "ApertureValue": ((b6(d[4]) << 6) | b6(d[5])) as f32 / 100.0,
            "ApertureRingTPosition": {
                "TNumber": ((b1(d[7] >> 6) << 7) | b7(d[6])) as f32 / 10.0,
                "Tenths": b4(d[7])
            },
            "FocalLength": ((b4(d[8]) << 6) | b6(d[9])) as f32, // mm, only for zoom lenses
            "HyperfocalDistance":     (b6(d[10]) << 18) | (b6(d[11]) << 12) | (b6(d[12]) << 6) | b6(d[13]), // mm
            "NearFocusDistance":      (b6(d[14]) << 18) | (b6(d[15]) << 12) | (b6(d[16]) << 6) | b6(d[17]), // mm
            "FarFocusDistance":       (b6(d[18]) << 18) | (b6(d[19]) << 12) | (b6(d[20]) << 6) | b6(d[21]), // mm
            "HorizontalFov":          ((b6(d[22]) << 6) | b6(d[23])) as f32 / 10.0,
            "EntrancePupilPosition":  ((b4(d[24]) << 6) | b6(d[25])) as i32 * if ((d[24] >> 5) & 1) == 1 { -1 } else { 1 },
        });
        if d[26] == b'S' {
            json.as_object_mut().unwrap().insert("SerialNumber".into(), String::from_utf8(d[27..27+9].to_vec()).unwrap().trim().into());
            *d = &d[27+9..];
        } else {
            json.as_object_mut().unwrap().insert("NormalizedZoomPosition".into(), (((b4(d[26]) << 6) | b6(d[27])) as f32 / 1000.0).into());
        }
        if d.len() >= 38 && d[28] == b'S' {
            json.as_object_mut().unwrap().insert("SerialNumber".into(), String::from_utf8(d[29..29+9].to_vec()).unwrap().trim().into());
            *d = &d[29+9..];
        }
        if d.len() >= 2 && &d[0..2] == &[0x0a, 0x0d] { *d = &d[2..]; }
        Some(serde_json::to_value(json).unwrap())
    } else {
        None
    }
}

pub fn parse(mut d: &[u8]) -> Option<Vec<serde_json::Value>> {
    if d.len() < 3 { return None; }

    // println!("Parse cooke: {}", pretty_hex::pretty_hex(&d));
    let mut values = Vec::new();

    loop {
        if d.is_empty() || d == [0x0a, 0x0d] { break; }
        match d[0] {
            b'N' if d[1] == b'N' => { // 5.1.35 NN: New (Optional) Start-up Command with Shading and Distortion Data
                // todo!()
                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                return None;
            },
            b'N' => { // 5.1.1 N: Fixed Data in ASCII Format
                let mut json = serde_json::json!({
                    "RecordType": "rt.header.lens.info"
                });
                let json = json.as_object_mut().unwrap();
                d = &d[1..];
                loop {
                    if d.is_empty() { break; }
                    match d[0] {
                        b'S' => { json.insert("SerialNumber".into(), String::from_utf8(d[1..10].to_vec()).unwrap().trim().into()); d = &d[10..]; },
                        b'O' => { json.insert("Owner".into(),        String::from_utf8(d[1..32].to_vec()).unwrap().trim().into()); d = &d[32..]; },
                        b'L' => { json.insert("LensType".into(),     if d[1] == b'Z' { "zoom" } else { "prime" }.into()); d = &d[2..]; },
                        b'N' | b'f' => { json.insert("MinFocalLength".into(), String::from_utf8(d[1..4].to_vec()).unwrap().trim_start_matches('0').parse::<u32>().unwrap().into()); d = &d[4..]; },
                        b'M' => { json.insert("MaxFocalLength".into(), String::from_utf8(d[1..4].to_vec()).unwrap().trim_start_matches('0').parse::<u32>().unwrap().into()); d = &d[4..]; },
                        b'U' => { json.insert("Units".into(),     if d[1] == b'I' || d[1] == b'B' { "imperial" } else { "metric" }.into()); d = &d[2..]; },
                        b'T' => { json.insert("TransmissionFactor".into(), (String::from_utf8(d[1..3].to_vec()).unwrap().parse::<f32>().unwrap() / 100.0).into()); d = &d[5..]; },
                        b'B' => { json.insert("FirmwareVersion".into(), String::from_utf8(d[1..5].to_vec()).unwrap().trim().into()); d = &d[5..]; },
                        _ => { break; }
                    }
                }
                values.push(serde_json::to_value(json).unwrap());
            },
            b'D' => { // 5.1.2 D: Pre-Defined Set of Calculated Data in ASCII Format
                // todo!()
                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                return None;
            },
            b'd' => { // 5.1.3 Kd: Packed Binary Data
                d = &d[1..];
                if let Some(json) = parse_kd(&mut d) {
                    values.push(serde_json::to_value(json).unwrap());
                }
            },
            b'i' => { // 5.1.28 Kdi: Lens plus Inertial Tracking Data
                let num_packets = (d.len() - 2 - 50) / 51;
                let _seq_num = d[1];
                d = &d[2..];
                let _size = ru16(&mut d);
                let mut k_d = &d[..38];
                if let Some(json) = parse_kd(&mut k_d) {
                    values.push(json);
                }
                d = &d[38..];
                let timestamp = ru16(&mut d);
                let (mx, my, mz) = (ri16(&mut d), ri16(&mut d), ri16(&mut d));
                values.push(serde_json::json!({
                    "RecordType": "rt.temporal.lens.magnetometer.raw",
                    "Timestamp": timestamp,
                    "Datavals": [{ "X": mx, "Y": my, "Z": mz }]
                }));
                for _i in 0..num_packets {
                    if d.is_empty() { break; }
                    let packet_type = d[0];
                    match packet_type {
                        1 | 2 => { // 1 - gyro, 2 - accelerometer
                            d = &d[1..];
                            let timestamp = ru16(&mut d);
                            let mut samples = Vec::new();
                            for _ in 0..8 {
                                let (x, y, z) = (ri16(&mut d), ri16(&mut d), ri16(&mut d));
                                samples.push(serde_json::json!({ "X": x, "Y": y, "Z": z }));
                                if packet_type == 1 { eprintln!("{_seq_num}\t{x}\t{y}\t{z}"); }
                            }
                            values.push(serde_json::json!({
                                "RecordType": if packet_type == 1 { "rt.temporal.lens.gyro.raw" } else { "rt.temporal.lens.accelerometer.raw" },
                                "Timestamp": timestamp,
                                "Datavals": samples
                            }));
                        },
                        0x0a if d.len() > 1 && d[1] == 0x0d => { break; }
                        _ => panic!("Invalid data: {}", pretty_hex::pretty_hex(&d)),
                    }
                }
                if d.len() >= 2 && &d[0..2] == &[0x0a, 0x0d] { d = &d[2..]; }
            },
            b'K' => {
                match d[1] {
                    b'3' => { // 5.1.4 K3: Name of Lens Manufacturer
                        // todo!()
                        log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                        return None;
                    },
                    b'4' => { // 5.1.5 K4: Name of Lens Type
                        // todo!()
                        log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                        return None;
                    },
                    b'6' if d[2] == b'1' => { // 5.1.29 K61: Inertial Calibration Coefficients
                        // todo!()
                        log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                        return None;
                    },
                    b'8' => { // 5.1.30 K8: Picture Width
                        // todo!()
                        log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                        return None;
                    },
                    b'9' if d[2] == b'1' => { // 5.1.31 K91: Anamorphic Squeeze Factor
                        // todo!()
                        log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                        return None;
                    },
                    b'K' => {
                        match d[2] {
                            b'i' if d[3] == b'd' => { // 5.1.34 KKid: Retrieve Lens Distortion Map and Shading Data
                                // todo!()
                                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                                return None;
                            },
                            b'i' => { // 5.1.32 KKi: Shading Data
                                // todo!()
                                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                                return None;
                            },
                            b'd' => { // 5.1.33 KKd: Distortion Map
                                // todo!()
                                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                                return None;
                            },
                            _ => {
                                panic!("Unknown Cooke d: {}", pretty_hex::pretty_hex(&d));
                            }
                        }
                    },
                    _ => {
                        println!("Unknown Cooke d: {}", pretty_hex::pretty_hex(&d));
                        return None;
                    }
                }
            },
            b'P' => { // 5.1.6 P: Lens Temperature
                // todo!()
                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                return None;
            },
            b'B' => { // 5.1.7 B: Firmware Version Number
                // todo!()
                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                return None;
            },
            b'O' => { // 5.1.23 OS: [EDSU] Current Channel Settings
                // todo!()
                log::error!("Cooke data not implemented: {}", pretty_hex::pretty_hex(&d));
                return None;
            },
            b'z' => { // ZEISS eXtended Data
                if d.len() >= 35 {
                    d = &d[1..];
                    let mut json = serde_json::json!({
                        "RecordType": "rt.temporal.zeiss.extended.data",
                        "FocusDistance": (b6(d[0]) << 18) | (b6(d[1]) << 12) | (b6(d[2]) << 6) | b6(d[3]),
                        "ApertureRingTPosition": {
                            "TNumber": ((b1(d[5] >> 6) << 7) | b7(d[4])) as f32 / 10.0,
                            "Tenths": b4(d[5])
                        },
                        "HorizontalFov":         ((b6(d[6]) << 6) | b6(d[7])) as f32 / 10.0,
                        "EntrancePupilPosition": ((b4(d[8]) << 6) | b6(d[9])) as i32 * if ((d[8] >> 5) & 1) == 1 { -1 } else { 1 }
                    });
                    d = &d[10..];
                    json.as_object_mut().unwrap().insert("ShadingData".into(),    (0..6).into_iter().map(|_| ri16(&mut d)).collect::<Vec<_>>().into());
                    json.as_object_mut().unwrap().insert("DistortionData".into(), (0..6).into_iter().map(|_| ri16(&mut d)).collect::<Vec<_>>().into());
                    if d.len() >= 2 && &d[0..2] == &[0x0a, 0x0d] { d = &d[2..]; }
                    values.push(serde_json::to_value(json).unwrap());
                }
            },
            0 => { break; }
            _ => {
                log::error!("Unknown Cooke data: {}", pretty_hex::pretty_hex(&d));
                return None;
            }
        }
    }
    if values.is_empty() { return None; }

    Some(values)
}
