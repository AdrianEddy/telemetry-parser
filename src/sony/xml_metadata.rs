// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2021 Adrian <adrian.eddy at gmail>

use quick_xml::events::Event;
use quick_xml::Reader;

#[allow(dead_code)]
pub fn parse_from_file(stream: &mut std::fs::File) -> std::io::Result<Metadata> {
    let ctx = mp4parse::read_mp4(stream)?;

    if let Some(Ok(md)) = ctx.metadata {
        if let Some(mp4parse::XmlBox::StringXmlBox(x)) = md.xml {
            let parsed_md = parse(&x[..]);
            log::info!("Model: {} {}, frames: {}", parsed_md.manufacturer, parsed_md.model, parsed_md.frame_count);
            return Ok(parsed_md);
        }
    }
    Err(std::io::ErrorKind::NotFound.into())
}

pub struct Metadata {
    pub manufacturer: String,
    pub model: String,
    pub frame_count: usize,
}

pub fn parse(data: &[u8]) -> Metadata {
    let mut reader = Reader::from_reader(data);
    let mut buf = Vec::new();

    let mut frame_count = 0usize;
    let mut model = String::new();
    let mut manufacturer = String::new();

    loop {
        match reader.read_event(&mut buf) {
            Ok(Event::Empty(ref e)) => {
                if e.name() == b"Duration" || e.name() == b"Device" {
                    for ox in e.attributes() {
                        if let Ok(x) = ox {
                            if x.key == b"value"        { frame_count = String::from_utf8_lossy(&x.value).parse::<usize>().unwrap(); }
                            if x.key == b"modelName"    { model = String::from_utf8_lossy(&x.value).into(); }
                            if x.key == b"manufacturer" { manufacturer = String::from_utf8_lossy(&x.value).into(); }
                        }
                    }
                }
            },
            Ok(Event::Eof) => break, // exits the loop when reaching end of file
            Err(e) => panic!("Error at position {}: {:?}", reader.buffer_position(), e),
            _ => (), // There are several other `Event`s we do not consider here
        }
        buf.clear();
    }
    Metadata { manufacturer, model, frame_count }
}
