// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use std::cell::*;
use std::rc::*;
use std::io::*;
use std::sync::{ Arc, atomic::AtomicBool };

use crate::tags_impl::*;
use crate::*;
use memchr::memmem;

mod binary;
mod csv;

#[derive(Default)]
pub struct BlackBox {
    pub model: Option<String>,
    csv: bool
}

impl BlackBox {
    pub fn camera_type(&self) -> String {
        "BlackBox".to_owned()
    }
    pub fn has_accurate_timestamps(&self) -> bool {
        false
    }
    pub fn possible_extensions() -> Vec<&'static str> {
        vec!["bfl", "bbl", "csv", "txt"]
    }
    pub fn frame_readout_time(&self) -> Option<f64> {
        None
    }
    pub fn normalize_imu_orientation(_: String) -> String {
        "ZYx".into()
    }

    pub fn detect<P: AsRef<std::path::Path>>(buffer: &[u8], _filepath: P, _options: &crate::InputOptions) -> Option<Self> {
        // BBL - container format, can contain multiple logs, each starting with "H Product:Blackbox flight data recorder by Nicholas Sherlock." and ending with "End of log\0"
        // BFL - single flight log file

        if memmem::find(buffer, b"H Product:Blackbox").is_some() {
            return Some(Self {
                model: util::find_between(buffer, b"H Firmware revision:", b'\n'),
                csv: false
            });
        }
        if memmem::find(buffer, b"\"loopIteration\",\"time\"").is_some() || memmem::find(buffer, b"loopIteration,time").is_some() {
            return Some(Self {
                model: util::find_between(buffer, b"\"Firmware revision\",\"", b'"'),
                csv: true
            });
        }
        None
    }

    pub fn parse<T: Read + Seek, F: Fn(f64)>(&mut self, stream: &mut T, size: usize, progress_cb: F, cancel_flag: Arc<AtomicBool>, options: crate::InputOptions) -> Result<Vec<SampleInfo>> {
        if self.csv {
            csv::parse(stream, size, progress_cb, cancel_flag, options)
        } else {
            binary::parse(stream, size, progress_cb, cancel_flag, options)
        }
    }

    fn parse_field_name(field: &str) -> FieldType {
        if let Some(pos) = field.find('[') {
            let idx = (&field[pos+1..pos+2]).parse::<u8>().unwrap();
            match &field[..pos] {
                "GPS_coord" |
                "GPS_home" => FieldType::Vector2(field[..pos].to_owned(), idx),

                "setpoint" |
                "rcCommand" |
                "rcCommands" => FieldType::Vector4(field[..pos].to_owned(), idx),
                "motor" |
                "debug" => FieldType::Vector8(field[..pos].to_owned(), idx),

                _ => FieldType::Vector3(field[..pos].to_owned(), idx)
            }
        } else {
            FieldType::Single(field.to_owned())
        }
    }

    fn tag_id(name: &str) -> TagId {
        match name {
            "gyroADC" |
            "accSmooth" => TagId::Data,

            _ => TagId::Custom(name.to_owned())
        }
    }
    fn group_from_key(name: &str) -> GroupId {
        match name {
            "gyroADC" => GroupId::Gyroscope,
            "accSmooth" => GroupId::Accelerometer,
            _ => GroupId::Custom(name.to_owned())
        }
    }

    fn prepare_vectors_from_headers(headers: &[&str]) -> Columns {
        let mut columns = Columns::default();
        macro_rules! insert_entry {
            ($c:expr, $name:expr, $entry_type:ident) => {
                // If it's a single item or first item of vector/array, create a new TagDescription and append it to the list
                // `descriptions` will have len() less than CSV headers count,
                // because columns like `gyroADC[1]` and `gyroADC[2]` will be stored as a single Vector3 in `gyroADC`, and not 3 separate floats
                if $c == 0 {
                    let group = Self::group_from_key(&$name);
                    let tag = Self::tag_id(&$name);

                    let tag_desc = tag!(parsed group, tag, $name, $entry_type, |v| format!("{:?}", v), Vec::new(), vec![]);

                    columns.descriptions.push(Rc::new(RefCell::new(tag_desc)));
                }

                // Take last created TagDescription and store the reference for it
                // `columns` will have len() equal to CSV headers count
                columns.columns.push(HeaderTagDesc {
                    index: $c,
                    desc: columns.descriptions.last_mut().unwrap().clone()
                });
            }
        }

        for x in headers {
            match Self::parse_field_name(&x) {
                FieldType::Single(ref hdr) => { insert_entry!(0, hdr, Vec_TimeScalar_i64); }
                FieldType::Vector2(ref hdr, c) => { insert_entry!(c, hdr, Vec_TimeArray2_f64); }
                FieldType::Vector3(ref hdr, c) => { insert_entry!(c, hdr, Vec_TimeVector3_f64); }
                FieldType::Vector4(ref hdr, c) => { insert_entry!(c, hdr, Vec_TimeArray4_f64); }
                FieldType::Vector8(ref hdr, c) => { insert_entry!(c, hdr, Vec_TimeArray8_f64); }
            }
        }

        columns
    }

    fn insert_value_to_vec(desc: &mut TagDescription, time: f64, val: f64, i: u8, gyro_only: bool) {
        if desc.group == GroupId::Gyroscope     && val.abs() > 3600.0   { log::warn!("Rejecting gyro {val}"); return; }
        if desc.group == GroupId::Accelerometer && val.abs() > 100000.0 { log::warn!("Rejecting accl {val}"); return; }

        if gyro_only && desc.group != GroupId::Gyroscope && desc.group != GroupId::Accelerometer { return; }

        match &mut desc.value {
            TagValue::Vec_TimeScalar_i64(vec) => {
                vec.get_mut().push(TimeScalar { t: time, v: val as i64 });
            },
            TagValue::Vec_TimeArray2_f64(vec) => match i {
                0 => vec.get_mut().push(TimeArray2 { t: time, v: [val as f64, 0.0] }),
                _ => vec.get_mut().last_mut().unwrap().v[i as usize] = val as f64,
            },
            TagValue::Vec_TimeVector3_f64(vec) => match i {
                0 => vec.get_mut().push(TimeVector3 { t: time, x: val as f64, ..Default::default() }),
                1 => vec.get_mut().last_mut().unwrap().y = val as f64,
                2 => vec.get_mut().last_mut().unwrap().z = val as f64,
                _ => { }
            },
            TagValue::Vec_TimeArray4_f64(vec) => match i {
                0 => vec.get_mut().push(TimeArray4 { t: time, v: [val as f64, 0.0, 0.0, 0.0] }),
                _ => vec.get_mut().last_mut().unwrap().v[i as usize] = val as f64,
            }
            TagValue::Vec_TimeArray8_f64(vec) => match i {
                0 => vec.get_mut().push(TimeArray8 { t: time, v: [val as f64, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] }),
                _ => vec.get_mut().last_mut().unwrap().v[i as usize] = val as f64,
            }
            _ => { panic!("Unknown field type"); }
        }
    }

}

#[derive(Debug)]
enum FieldType {
    Single(String),
    Vector2(String, u8),
    Vector3(String, u8),
    Vector4(String, u8),
    Vector8(String, u8)
}
struct HeaderTagDesc {
    index: u8,
    desc: Rc<RefCell<TagDescription>>
}
#[derive(Default)]
struct Columns {
    columns: Vec<HeaderTagDesc>,
    descriptions: Vec<Rc<RefCell<TagDescription>>>
}
