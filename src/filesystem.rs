// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright © 2023 Adrian <adrian.eddy at gmail>

use std::io::Read;
use std::sync::OnceLock;

#[cfg(target_os = "android")]
mod base {
    pub type FilesystemBase = jni::JavaVM;
    pub fn get_base() -> FilesystemBase { unsafe { jni::JavaVM::from_raw(ndk_context::android_context().vm().cast()) }.unwrap() }
}
#[cfg(not(target_os = "android"))]
mod base {
    pub type FilesystemBase = ();
    pub fn get_base() -> FilesystemBase { () }
}
pub use base::*;

static FILESYSTEM_FUNCTIONS: OnceLock<FilesystemFunctions> = OnceLock::new();

#[derive(Debug)]
pub struct FilesystemFunctions {
    pub get_filename: fn(&str) -> String,
    pub get_folder: fn(&str) -> String,
    pub list_folder: fn(&str) -> Vec<(String, String)>, // -> Vec<(name, path)>
    pub open_file: for<'a> fn(&'a FilesystemBase, &str) -> std::io::Result<FileWrapper<'a>>,
}

pub unsafe fn set_filesystem_functions<'a>(functions: FilesystemFunctions) {
    FILESYSTEM_FUNCTIONS.set(functions).expect("Functions can be set only once");
}

pub fn get_filename(path: &str) -> String {
    if let Some(funcs) = FILESYSTEM_FUNCTIONS.get() {
        return (funcs.get_filename)(path);
    }
    let mut filename = path;
    if let Some(pos) = path.rfind('/').or_else(|| path.rfind('\\')) {
        filename = &path[pos + 1..];
    }
    filename.to_owned()
}

pub fn get_folder(path: &str) -> String {
    if let Some(funcs) = FILESYSTEM_FUNCTIONS.get() {
        return (funcs.get_folder)(path);
    }
    if !path.ends_with('/') && !path.ends_with('\\') {
        if let Some(pos) = path.rfind('/').or_else(|| path.rfind('\\')) {
            return path[..pos].to_owned();
        }
    }
    path.to_owned()
}

pub fn file_with_extension(path: &str, ext: &str) -> Option<String> {
    if let Some(pos) = path.rfind('.') {
        let new_path = if ext.is_empty() { path[..pos].to_owned() } else { format!("{}.{}", &path[..pos], ext) };
        if std::path::Path::new(&new_path).exists() {
            return Some(new_path);
        }
    }
    // fallback
    let filename = get_filename(path);
    if let Some(pos) = filename.rfind('.') {
        let files = list_folder(get_folder(path).as_str());
        let new_name = if ext.is_empty() { filename[..pos].to_owned() } else { format!("{}.{}", &filename[..pos], ext) };
        if let Some(fpath) = files.iter().find_map(|(name, path)| if name == &new_name { Some(path) } else { None }) {
            return Some(fpath.into());
        }
    }
    None
}

pub fn list_folder(path: &str) -> Vec<(String, String)> {
    if let Some(funcs) = FILESYSTEM_FUNCTIONS.get() {
        return (funcs.list_folder)(path);
    }
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries {
            if let Ok(entry) = entry {
                if entry.metadata().as_ref().map(|x| x.is_file() || x.is_dir()).unwrap_or_default() {
                    files.push((entry.file_name().to_string_lossy().to_string(), entry.path().to_string_lossy().to_string()));
                }
            }
        }
    }
    files
}
pub trait ReadSeek: std::io::Read + std::io::Seek {}
impl<T: std::io::Read + std::io::Seek> ReadSeek for T {}

pub struct FileWrapper<'a> {
    pub file: Box<dyn ReadSeek + 'a>,
    pub size: usize,
}
pub fn open_file<'a>(_base: &'a FilesystemBase, path: &str) -> std::io::Result<FileWrapper<'a>> {
    if let Some(funcs) = FILESYSTEM_FUNCTIONS.get() {
        return (funcs.open_file)(_base, path);
    }
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len() as usize;
    Ok(FileWrapper { file: Box::new(file), size })
}

pub fn get_extension(path: &str) -> String {
    let filename = get_filename(path);
    if let Some(pos) = filename.rfind('.') {
        return filename[pos + 1..].to_ascii_lowercase();
    }
    Default::default()
}

pub fn read_file(path: &str) -> std::io::Result<Vec<u8>> {
    let base = get_base();
    let mut wrapper = open_file(&base, path)?;
    let mut bytes = Vec::with_capacity(wrapper.size);
    wrapper.file.read_to_end(&mut bytes)?;
    Ok(bytes)
}
