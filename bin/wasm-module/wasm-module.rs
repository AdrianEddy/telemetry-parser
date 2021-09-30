use wasm_bindgen::prelude::*;
use std::collections::BTreeMap;

use telemetry_parser::*;

#[wasm_bindgen]
pub struct Parser {
    camera: Option<String>,
    model: Option<String>,
    input: Input
}

#[wasm_bindgen]
impl Parser {
    #[wasm_bindgen(constructor)]
    pub fn new(data: &[u8], filename: &str) -> Result<Parser, JsValue> {
        let mut stream = std::io::Cursor::new(&data);

        let input = Input::from_stream(&mut stream, data.len(), filename).map_err(Self::err)?;

        Ok(Self {
            camera: Some(input.camera_type()),
            model: input.camera_model().map(String::clone),
            input: input,
        })
    }

    pub fn telemetry(&self, human_readable: Option<bool>) -> Result<JsValue, JsValue> {
        if self.input.samples.is_none() { return Err(JsValue::from("No metadata")); }

        let samples = self.input.samples.as_ref().unwrap();
        let mut output = Vec::with_capacity(samples.len());

        for info in samples {
            if info.tag_map.is_none() { continue; }

            let mut groups = BTreeMap::new();
            let groups_map = info.tag_map.as_ref().unwrap();

            for (group, map) in groups_map {
                let group_map = groups.entry(group).or_insert_with(BTreeMap::new);
                for (tagid, info) in map {
                    let value = if human_readable.unwrap_or(false) {
                        serde_json::to_value(info.value.to_string())
                    } else {
                        serde_json::to_value(info.value.clone())
                    }.unwrap();
                    group_map.insert(tagid, value);
                }
            }

            output.push(groups);
        }

        Ok(JsValue::from_serde(&output).unwrap())
    }

    pub fn normalized_imu(&self, orientation: Option<String>) -> Result<JsValue, JsValue> {
        if self.input.samples.is_none() { return Err(JsValue::from("No metadata")); }

        let imu_data = util::normalized_imu(&self.input, orientation).map_err(Self::err)?;
        
        Ok(JsValue::from_serde(&imu_data).unwrap())
    }

    fn err(e: std::io::Error) -> JsValue {
        JsValue::from(format!("IO error {:?}", e))
    }
    
    #[wasm_bindgen(getter)]
    pub fn camera(&self) -> Option<String> { self.camera.clone() }

    #[wasm_bindgen(getter)]
    pub fn model(&self) -> Option<String> { self.model.clone() }
}
