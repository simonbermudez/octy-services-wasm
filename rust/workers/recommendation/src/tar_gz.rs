//! In-memory `.tar.gz` extraction for the SageMaker `model.tar.gz` artifact
//! (replaces Python `tarfile.open(mode="r:gz")`). Pure logic — no spin-sdk.

use anyhow::{anyhow, bail, Result};
use std::io::Read;

pub struct TarEntry {
    pub name: String,
    pub data: Vec<u8>,
}

/// Gunzip + untar an in-memory archive, returning the regular-file members.
pub fn extract_tar_gz(bytes: &[u8]) -> Result<Vec<TarEntry>> {
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut tar = Vec::new();
    decoder
        .read_to_end(&mut tar)
        .map_err(|e| anyhow!("Error occurred when downloading and decompressing file -- {e}"))?;
    extract_tar(&tar)
}

fn extract_tar(tar: &[u8]) -> Result<Vec<TarEntry>> {
    let mut entries = Vec::new();
    let mut offset = 0usize;
    let mut pending_long_name: Option<String> = None;

    while offset + 512 <= tar.len() {
        let header = &tar[offset..offset + 512];
        offset += 512;
        if header.iter().all(|&b| b == 0) {
            break; // end-of-archive zero block
        }

        let size = parse_octal(&header[124..136])?;
        let padded = size.div_ceil(512) * 512;
        if offset + size > tar.len() {
            bail!("truncated tar archive");
        }
        let data = &tar[offset..offset + size];
        let typeflag = header[156];

        match typeflag {
            // GNU long-name entry: the data block is the next member's name.
            b'L' => {
                pending_long_name = Some(cstr(data).to_string());
            }
            // pax extended headers: honour a `path=` override, if present.
            b'x' | b'g' => {
                if let Some(path) = parse_pax_path(data) {
                    pending_long_name = Some(path);
                }
            }
            // regular file
            b'0' | 0 => {
                let name = match pending_long_name.take() {
                    Some(name) => name,
                    None => {
                        let name = cstr(&header[0..100]).to_string();
                        let prefix = cstr(&header[345..500]);
                        if !prefix.is_empty() {
                            format!("{prefix}/{name}")
                        } else {
                            name
                        }
                    }
                };
                entries.push(TarEntry {
                    name,
                    data: data.to_vec(),
                });
            }
            // directories, links, … — skipped (tarfile.extractfile returns None)
            _ => {
                pending_long_name = None;
            }
        }

        offset += padded;
    }

    Ok(entries)
}

fn cstr(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    std::str::from_utf8(&bytes[..end]).unwrap_or("").trim()
}

fn parse_octal(bytes: &[u8]) -> Result<usize> {
    let text = cstr(bytes);
    if text.is_empty() {
        return Ok(0);
    }
    usize::from_str_radix(text, 8).map_err(|e| anyhow!("bad tar size field {text:?}: {e}"))
}

fn parse_pax_path(data: &[u8]) -> Option<String> {
    // pax records: "<len> <keyword>=<value>\n"
    let text = std::str::from_utf8(data).ok()?;
    for record in text.split('\n') {
        if let Some((_, rest)) = record.split_once(' ') {
            if let Some((key, value)) = rest.split_once('=') {
                if key == "path" {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

/// Basename match — tar members may be stored as `./name` or `dir/name`.
pub fn entry_basename(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}
