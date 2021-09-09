use pyo3::prelude::*;
use std::collections::BTreeMap;
use pythonize::pythonize;

use telemetry_parser::*;

#[pyclass]
struct Parser {
    #[pyo3(get, set)]
    camera: Option<String>,
    #[pyo3(get, set)]
    model: Option<String>,
    input: Input
}

#[pymethods]
impl Parser {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let mut stream = std::fs::File::open(&path)?;
        let filesize = stream.metadata()?.len() as usize;

        let input = Input::from_stream(&mut stream, filesize)?;

        Ok(Self {
            camera: Some(input.camera_type()),
            model: input.camera_model().map(String::clone),
            input: input,
        })
    }

    fn telemetry(&self, human_readable: Option<bool>) -> PyResult<Py<PyAny>> {
        if self.input.samples.is_none() { return Err(pyo3::exceptions::PyValueError::new_err("No metadata")); }

        let samples = self.input.samples.as_ref().unwrap();
        let mut output = Vec::with_capacity(samples.len());

        for info in samples {
            if info.tag_map.is_none() { continue; }

            let mut groups = BTreeMap::new();
            let groups_map = info.tag_map.as_ref().unwrap();

            for (group, map) in groups_map {
                let group_map = groups.entry(group).or_insert(BTreeMap::new());
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

        let gil = Python::acquire_gil();
        Ok(pythonize(gil.python(), &output)?)
    }

    fn normalized_imu(&self, orientation: Option<String>) -> PyResult<Py<PyAny>> {
        if self.input.samples.is_none() { return Err(pyo3::exceptions::PyValueError::new_err("No metadata")); }

        let samples = self.input.samples.as_ref().unwrap();

        let imu_data = util::normalized_imu(&samples, orientation)?;
        
        let gil = Python::acquire_gil();
        Ok(pythonize(gil.python(), &imu_data)?)
    }
}

#[pymodule]
fn telemetry_parser(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_class::<Parser>()?;

    Ok(())
}
