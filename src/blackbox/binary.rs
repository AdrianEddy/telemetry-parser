use std::rc::*;
use std::io::*;

use crate::tags_impl::*;
use crate::*;
use fc_blackbox::BlackboxRecord;
use fc_blackbox::MultiSegmentBlackboxReader;

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
    let mut samples = Vec::new();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;

    for mut bbox in MultiSegmentBlackboxReader::from_bytes(&bytes).successful_only() {
        // Remove acc_1G from `other_headers` because we will have it in Accelerometer/Scale tag, instead of in metadata
        let accl_scale = bbox.header.other_headers.remove("acc_1G").unwrap_or("1.0".to_owned()).parse::<f64>().unwrap();
        let gyro_scale = bbox.header.raw_gyro_scale as f64;

        let mut map = GroupedTagMap::new();

        util::insert_tag(&mut map, tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), {
            serde_json::to_value(&bbox.header.other_headers).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?
        }, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]));
        util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]));

        let headers = bbox.header.ip_fields_in_order.iter().map(|x| x.name.as_str()).collect::<Vec<&str>>();
        let mut column_struct = super::BlackBox::prepare_vectors_from_headers(&headers);
        
        while let Some(record) = bbox.next() {
            match record {
                BlackboxRecord::Main(values) => {
                    let time = values[1] as f64 / 1_000_000.0;
                    for (col, &value) in column_struct.columns.iter().zip(values) {
                        let mut desc = col.desc.as_ref().borrow_mut();
                        super::BlackBox::insert_value_to_vec(&mut desc, time, value as f64, col.index);
                    }
                }
                BlackboxRecord::Event(fc_blackbox::frame::event::Frame::EndOfLog) => {
                    break;
                }
                _ => {}
            }
        }
        drop(column_struct.columns); // Release all weak pointers

        // Add filled vectors to the tag map
        for desc in column_struct.descriptions.drain(..) {
            let desc = Rc::try_unwrap(desc).unwrap().into_inner();
            util::insert_tag(&mut map, desc);
        }

        samples.push(SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) });
    }

    Ok(samples)
}
