// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use pyo3::prelude::*;
use pyo3::sync::MutexExt;
use std::collections::BTreeMap;
use pythonize::pythonize;
use std::sync::{ Arc, Mutex, atomic::AtomicBool };

use ::telemetry_parser::*;

#[pyclass]
struct Parser {
    #[pyo3(get, set)]
    camera: Option<String>,
    #[pyo3(get, set)]
    model: Option<String>,
    input: Mutex<Input>
}

#[pymethods]
impl Parser {
    #[new]
    fn new(path: &str) -> PyResult<Self> {
        let mut stream = std::fs::File::open(&path)?;
        let filesize = stream.metadata()?.len() as usize;

        let input = Input::from_stream(&mut stream, filesize, &path, |_|(), Arc::new(AtomicBool::new(false)))?;

        Ok(Self {
            camera: Some(input.camera_type()),
            model: input.camera_model().map(String::clone),
            input: Mutex::new(input),
        })
    }

    #[pyo3(signature = (human_readable=None))]
    fn telemetry<'py>(&self, py: Python<'py>, human_readable: Option<bool>) -> PyResult<Bound<'py, PyAny>> {
        let input = self.input.lock_py_attached(py).unwrap();
        if input.samples.is_none() { return Err(pyo3::exceptions::PyValueError::new_err("No metadata")); }

        let samples = input.samples.as_ref().unwrap();
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

        Ok(pythonize(py, &output)?)
    }

    #[pyo3(signature = (orientation=None))]
    fn normalized_imu<'py>(&self, py: Python<'py>, orientation: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let input = self.input.lock_py_attached(py).unwrap();
        if input.samples.is_none() { return Err(pyo3::exceptions::PyValueError::new_err("No metadata")); }

        let imu_data = util::normalized_imu(&input, orientation)?;

        Ok(pythonize(py, &imu_data)?)
    }
}

#[pymodule]
fn telemetry_parser(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Parser>()?;

    Ok(())
}
