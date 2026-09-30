//! A minimal zip writer and reader (deflate or stored entries, no zip64, no encryption), built
//! on flate2 so the crate needs no zip dependency. Enough for PCB Studio's export (two text
//! files in a folder) and for reading such archives back.

use std::io::{Read, Write};

use flate2::Crc;
use flate2::Compression;
use flate2::read::DeflateDecoder;
use flate2::write::DeflateEncoder;

use crate::parse::IdfError;

/// MS-DOS time and date for a `yyyy/mm/dd.hh:mm:ss` string (1980-01-01 if it doesn't parse).
pub fn dos_datetime(date: &str) -> (u16, u16) {
    let nums: Vec<u32> = date.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
    if let [y, mo, d, h, mi, s, ..] = nums[..]
        && (1980..=2107).contains(&y)
        && (1..=12).contains(&mo)
        && (1..=31).contains(&d)
        && h < 24
        && mi < 60
        && s < 60
    {
        let time = ((h << 11) | (mi << 5) | (s / 2)) as u16;
        let date = (((y - 1980) << 9) | (mo << 5) | d) as u16;
        return (time, date);
    }
    (0, (1 << 5) | 1)
}

fn crc32(data: &[u8]) -> u32 {
    let mut c = Crc::new();
    c.update(data);
    c.sum()
}

/// Build a zip archive from `(path, bytes)` entries, deflate-compressed, all stamped with the
/// given DOS time/date.
pub fn build_zip(entries: &[(String, Vec<u8>)], dos: (u16, u16)) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for (name, data) in entries {
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        enc.write_all(data).expect("writing to a Vec cannot fail");
        let comp = enc.finish().expect("writing to a Vec cannot fail");
        let crc = crc32(data);
        let offset = out.len() as u32;
        let name_b = name.as_bytes();
        let common = |v: &mut Vec<u8>| {
            v.extend_from_slice(&20u16.to_le_bytes()); // version needed
            v.extend_from_slice(&0x0800u16.to_le_bytes()); // UTF-8 names
            v.extend_from_slice(&8u16.to_le_bytes()); // deflate
            v.extend_from_slice(&dos.0.to_le_bytes());
            v.extend_from_slice(&dos.1.to_le_bytes());
            v.extend_from_slice(&crc.to_le_bytes());
            v.extend_from_slice(&(comp.len() as u32).to_le_bytes());
            v.extend_from_slice(&(data.len() as u32).to_le_bytes());
            v.extend_from_slice(&(name_b.len() as u16).to_le_bytes());
            v.extend_from_slice(&0u16.to_le_bytes()); // extra length
        };
        out.extend_from_slice(&0x04034b50u32.to_le_bytes());
        common(&mut out);
        out.extend_from_slice(name_b);
        out.extend_from_slice(&comp);

        central.extend_from_slice(&0x02014b50u32.to_le_bytes());
        central.extend_from_slice(&20u16.to_le_bytes()); // version made by
        common(&mut central);
        central.extend_from_slice(&0u16.to_le_bytes()); // comment length
        central.extend_from_slice(&0u16.to_le_bytes()); // disk number
        central.extend_from_slice(&0u16.to_le_bytes()); // internal attributes
        central.extend_from_slice(&0u32.to_le_bytes()); // external attributes
        central.extend_from_slice(&offset.to_le_bytes());
        central.extend_from_slice(name_b);
    }
    let cd_offset = out.len() as u32;
    out.extend_from_slice(&central);
    out.extend_from_slice(&0x06054b50u32.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(central.len() as u32).to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn err(m: &str) -> IdfError {
    IdfError::new(0, format!("zip: {m}"))
}

fn u16_at(b: &[u8], i: usize) -> Result<u16, IdfError> {
    b.get(i..i + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| err("truncated archive"))
}

fn u32_at(b: &[u8], i: usize) -> Result<u32, IdfError> {
    b.get(i..i + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| err("truncated archive"))
}

/// Read every file entry of a zip archive as `(path, bytes)`. Directory entries are skipped.
pub fn zip_entries(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, IdfError> {
    if bytes.len() < 22 {
        return Err(err("not a zip archive"));
    }
    let eocd = (0..=bytes.len() - 22)
        .rev()
        .find(|&i| bytes[i..i + 4] == 0x06054b50u32.to_le_bytes())
        .ok_or_else(|| err("no end-of-central-directory record"))?;
    let count = u16_at(bytes, eocd + 10)? as usize;
    let mut p = u32_at(bytes, eocd + 16)? as usize;
    let mut out = Vec::new();
    for _ in 0..count {
        if u32_at(bytes, p)? != 0x02014b50 {
            return Err(err("bad central directory entry"));
        }
        let method = u16_at(bytes, p + 10)?;
        let crc = u32_at(bytes, p + 16)?;
        let csize = u32_at(bytes, p + 20)? as usize;
        let usize_ = u32_at(bytes, p + 24)? as usize;
        let nlen = u16_at(bytes, p + 28)? as usize;
        let xlen = u16_at(bytes, p + 30)? as usize;
        let clen = u16_at(bytes, p + 32)? as usize;
        let local = u32_at(bytes, p + 42)? as usize;
        let name = String::from_utf8_lossy(bytes.get(p + 46..p + 46 + nlen).ok_or_else(|| err("truncated name"))?).into_owned();
        p += 46 + nlen + xlen + clen;
        if name.ends_with('/') {
            continue;
        }
        if u32_at(bytes, local)? != 0x04034b50 {
            return Err(err("bad local header"));
        }
        let start = local + 30 + u16_at(bytes, local + 26)? as usize + u16_at(bytes, local + 28)? as usize;
        let comp = bytes.get(start..start + csize).ok_or_else(|| err("truncated entry data"))?;
        let data = match method {
            0 => comp.to_vec(),
            8 => {
                let mut v = Vec::with_capacity(usize_);
                DeflateDecoder::new(comp).read_to_end(&mut v).map_err(|e| err(&format!("inflate {name}: {e}")))?;
                v
            }
            m => return Err(err(&format!("unsupported compression method {m} for {name}"))),
        };
        if crc32(&data) != crc {
            return Err(err(&format!("CRC mismatch in {name}")));
        }
        out.push((name, data));
    }
    Ok(out)
}
