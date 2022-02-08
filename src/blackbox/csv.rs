use std::collections::BTreeMap;
use std::rc::*;
use std::io::*;

use crate::tags_impl::*;
use crate::*;

pub fn parse<T: Read + Seek>(stream: &mut T, _size: usize) -> Result<Vec<SampleInfo>> {
    let mut metadata = BTreeMap::new();

    let mut headers = None;

    let mut csv = csv::ReaderBuilder::new()
        .has_headers(false)
        .flexible(true)
        .trim(csv::Trim::All)
        .from_reader(stream);
    for row in csv.records() {
        let row = row?;
        if row.len() == 2 {
            metadata.insert(row[0].to_owned(), row[1].to_owned());
            continue;
        }
        if &row[0] == "loopIteration" {
            let hdrlist = row.iter().collect::<Vec<&str>>();
            headers = Some(super::BlackBox::prepare_vectors_from_headers(&hdrlist));
            continue;
        }
        if let Some(ref h) = headers {
            let time = row[1].parse::<i64>().unwrap() as f64 / 1000000.0;
            for (col, value) in h.columns.iter().zip(row.iter()) {
                let mut desc = col.desc.as_ref().borrow_mut();
                if let Ok(f) = value.parse::<f64>() {
                    super::BlackBox::insert_value_to_vec(&mut desc, time, f, col.index);
                } else {
                    super::BlackBox::insert_value_to_vec(&mut desc, time, f64::NAN, col.index);
                    // eprintln!("Invalid float {}", value);
                }
            }
        }
    }

    let mut map = GroupedTagMap::new();

    // Remove from `metadata` because we will have it in the Scale tag
    let accl_scale = metadata.remove("acc_1G")    .unwrap_or("1.0".to_owned()).parse::<f64>().unwrap();
    let gyro_scale = metadata.remove("gyro_scale").unwrap_or("1.0".to_owned()).parse::<f64>().unwrap();

    util::insert_tag(&mut map, 
        tag!(parsed GroupId::Default, TagId::Metadata, "Extra metadata", Json, |v| format!("{:?}", v), serde_json::to_value(metadata).map_err(|_| Error::new(ErrorKind::Other, "Serialize error"))?, vec![])
    );
    
    util::insert_tag(&mut map, tag!(parsed GroupId::Gyroscope,     TagId::Scale, "Gyroscope scale",     f64, |v| format!("{:?}", v), gyro_scale, vec![]));
    util::insert_tag(&mut map, tag!(parsed GroupId::Accelerometer, TagId::Scale, "Accelerometer scale", f64, |v| format!("{:?}", v), accl_scale, vec![]));
    
    if let Some(mut column_struct) = headers {
        drop(column_struct.columns); // Release all weak pointers

        // Add filled vectors to the tag map
        for desc in column_struct.descriptions.drain(..) {
            if let Ok(desc) = Rc::try_unwrap(desc) {
                util::insert_tag(&mut map, desc.into_inner());
            }
        }

        Ok(vec![
            SampleInfo { index: 0, timestamp_ms: 0.0, duration_ms: 0.0, tag_map: Some(map) }
        ])
    } else {
        Err(ErrorKind::InvalidInput.into())
    }
}
