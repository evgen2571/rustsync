use crate::error::EncError;
use std::{
    fs::File,
    io::{Error, Read},
    path::Path,
};

pub fn encrypt(
    file_path: impl AsRef<Path>,
    key_id: &String,
    nonce: &String,
) -> Result<File, EncError> {
    let mut file = File::open(file_path)?;
    let mut text = Vec::new();
    file.read_to_end(&mut text)?;

    // let mut output_file = File::create()?;
    Ok(file)
}
