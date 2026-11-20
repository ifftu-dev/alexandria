//! A zip writer that does what it is told, including lying.
//!
//! Shared by the package import tests through `#[path]`; not every test
//! binary tells every lie.
#![allow(dead_code)]

use std::io::Write;

pub struct Entry {
    name: Vec<u8>,
    data: Vec<u8>,
    deflate: bool,
    unix_mode: Option<u32>,
    declared_size: Option<u32>,
    method: Option<u16>,
    encrypted: bool,
}

pub fn file(name: &str, data: impl AsRef<[u8]>) -> Entry {
    Entry {
        name: name.as_bytes().to_vec(),
        data: data.as_ref().to_vec(),
        deflate: false,
        unix_mode: None,
        declared_size: None,
        method: None,
        encrypted: false,
    }
}

impl Entry {
    pub fn deflated(mut self) -> Self {
        self.deflate = true;
        self
    }

    /// Unix file type and permission bits, as `st_mode` holds them.
    pub fn unix_mode(mut self, mode: u32) -> Self {
        self.unix_mode = Some(mode);
        self
    }

    /// State this uncompressed size in both headers, whatever the data is.
    pub fn declaring_size(mut self, size: u32) -> Self {
        self.declared_size = Some(size);
        self
    }

    /// Claim this compression method, whatever was done to the data.
    pub fn claiming_method(mut self, method: u16) -> Self {
        self.method = Some(method);
        self
    }

    /// Set the flag that says the data is encrypted.
    pub fn flagged_encrypted(mut self) -> Self {
        self.encrypted = true;
        self
    }
}

pub fn zip(entries: &[Entry]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for entry in entries {
        let mut crc = flate2::Crc::new();
        crc.update(&entry.data);
        let stored = if entry.deflate {
            let mut encoder =
                flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
            encoder.write_all(&entry.data).unwrap();
            encoder.finish().unwrap()
        } else {
            entry.data.clone()
        };
        let method = entry.method.unwrap_or(if entry.deflate { 8 } else { 0 });
        // Bit 11: names are UTF-8. Bit 0: the data is encrypted.
        let flags: u16 = 0x0800 | u16::from(entry.encrypted);
        let size = entry.declared_size.unwrap_or(entry.data.len() as u32);
        let offset = out.len() as u32;

        let mut shared = Vec::new();
        shared.extend_from_slice(&20u16.to_le_bytes()); // version needed
        shared.extend_from_slice(&flags.to_le_bytes());
        shared.extend_from_slice(&method.to_le_bytes());
        shared.extend_from_slice(&0u16.to_le_bytes()); // time
        shared.extend_from_slice(&0x21u16.to_le_bytes()); // date: 1980-01-01
        shared.extend_from_slice(&crc.sum().to_le_bytes());
        shared.extend_from_slice(&(stored.len() as u32).to_le_bytes());
        shared.extend_from_slice(&size.to_le_bytes());
        shared.extend_from_slice(&(entry.name.len() as u16).to_le_bytes());
        shared.extend_from_slice(&0u16.to_le_bytes()); // extra field length

        out.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        out.extend_from_slice(&shared);
        out.extend_from_slice(&entry.name);
        out.extend_from_slice(&stored);

        directory.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
        directory.extend_from_slice(&((3u16 << 8) | 20).to_le_bytes()); // made on Unix
        directory.extend_from_slice(&shared);
        directory.extend_from_slice(&0u16.to_le_bytes()); // comment length
        directory.extend_from_slice(&0u16.to_le_bytes()); // disk number
        directory.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        directory.extend_from_slice(&(entry.unix_mode.unwrap_or(0o100644) << 16).to_le_bytes());
        directory.extend_from_slice(&offset.to_le_bytes());
        directory.extend_from_slice(&entry.name);
    }
    let directory_offset = out.len() as u32;
    out.extend_from_slice(&directory);
    out.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(directory.len() as u32).to_le_bytes());
    out.extend_from_slice(&directory_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}
