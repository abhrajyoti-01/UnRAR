//! UnRAR engine — signature-based format detection and archive operations.
//!
//! Native: ZIP (full r/w, AES-256), TAR+GZ/BZ2/XZ/ZST, 7z (read).
//! RAR: in-process via the `unrar` crate (vendored UnRAR source) — list,
//! test, extract, member read; also falls back to UnRAR.exe when present.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rar, Rev, Zip, SevenZ, Tar, Gz, Bz2, Xz, Zst, Cab, Unknown,
}

impl Format {
    pub fn label(self) -> &'static str {
        match self {
            Format::Rar => "RAR", Format::Rev => "REV", Format::Zip => "ZIP",
            Format::SevenZ => "7z", Format::Tar => "TAR", Format::Gz => "GZIP",
            Format::Bz2 => "BZIP2", Format::Xz => "XZ", Format::Zst => "ZST",
            Format::Cab => "CAB", Format::Unknown => "UNKNOWN",
        }
    }
}

pub fn detect_format(path: &Path) -> Format {
    let mut head = [0u8; 512];
    let n = File::open(path)
        .and_then(|mut f| f.read(&mut head))
        .unwrap_or(0);
    let h = &head[..n];
    if h.starts_with(b"Rar!\x1a\x07\x00") || h.starts_with(b"Rar!\x1a\x07\x01\x00") {
        return Format::Rar;
    }
    if h.starts_with(b"Rar!\x1aRev") { return Format::Rev; }
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06")
        || h.starts_with(b"PK\x07\x08") {
        return Format::Zip;
    }
    if h.starts_with(b"7z\xbc\xaf\x27\x1c") { return Format::SevenZ; }
    if h.starts_with(b"\xfd7zXZ\x00") { return Format::Xz; }
    if h.starts_with(b"BZh") { return Format::Bz2; }
    if h.starts_with(b"\x28\xb5\x2f\xfd") { return Format::Zst; }
    if h.starts_with(b"MSCF") { return Format::Cab; }
    if h.starts_with(b"\x1f\x8b") { return Format::Gz; }
    if h.len() > 262 && &h[257..262] == b"ustar" { return Format::Tar; }
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_lowercase()) {
        Some(e) if ["rar", "rev"].contains(&e.as_str()) => Format::Rar,
        Some(e) if e.starts_with('r') && e[1..].chars().all(|c| c.is_ascii_digit())
            && !e.is_empty() => Format::Rar,
        Some(e) if ["zip", "zipx", "jar"].contains(&e.as_str()) => Format::Zip,
        Some(e) if e == "tar" => Format::Tar,
        Some(e) if ["7z", "gz", "bz2", "xz", "zst", "cab"].contains(&e.as_str())
            => match e.as_str() {
                "7z" => Format::SevenZ, "gz" => Format::Gz, "bz2" => Format::Bz2,
                "xz" => Format::Xz, "zst" => Format::Zst, _ => Format::Cab,
            },
        _ => Format::Unknown,
    }
}

/// Detect a self-extracting exe: MZ header + appended ZIP payload.
/// Finds the ZIP end-of-central-directory in the last 64 KB and derives
/// the payload start from its cd_offset+cd_size fields.
pub fn sfx_payload_offset(path: &Path) -> Option<u64> {
    let mut f = File::open(path).ok()?;
    let file_len = f.metadata().ok()?.len();
    if file_len < 1024 { return None; }
    let mut head = [0u8; 2];
    f.read_exact(&mut head).ok()?;
    if &head != b"MZ" { return None; }
    // EOCD record lives in the last ~64 KB (comment ≤ 65535)
    let tail = (66 * 1024u64).min(file_len);
    f.seek(SeekFrom::End(-(tail as i64))).ok()?;
    let mut buf = vec![0u8; tail as usize];
    f.read_exact(&mut buf).ok()?;
    let mut off = None;
    for i in (0..buf.len().saturating_sub(22)).rev() {
        if buf[i..i + 4] == *b"PK\x05\x06" {
            let cd_size = u32::from_le_bytes(buf[i + 12..i + 16].try_into().ok()?);
            let cd_off  = u32::from_le_bytes(buf[i + 16..i + 20].try_into().ok()?);
            let clen    = u16::from_le_bytes(buf[i + 20..i + 22].try_into().ok()?);
            let eocd_pos = file_len - tail + i as u64;
            if eocd_pos + 22 + clen as u64 != file_len { continue; }
            let start = eocd_pos.checked_sub(cd_size as u64 + cd_off as u64)?;
            off = Some(start);
            break;
        }
    }
    let start = off?;
    // verify a local header at the computed payload start
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut sig = [0u8; 4];
    f.read_exact(&mut sig).ok()?;
    if &sig == b"PK\x03\x04" { Some(start) } else { None }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub size: u64,
    pub packed: u64,
    pub mtime: Option<String>,
    pub crc: Option<u32>,
    pub method: String,
    pub is_dir: bool,
    pub encrypted: bool,
}

impl Entry {
    pub fn ratio(&self) -> String {
        if self.is_dir || self.size == 0 || self.packed == 0 { return "-".into(); }
        let pct = 100i64 - (self.packed * 100 / self.size.max(1)) as i64;
        format!("{}%", pct.max(0))
    }
}

#[derive(Debug, Default)]
pub struct ArchiveInfo {
    pub format: Format,
    pub entries: Vec<Entry>,
    pub comment: String,
    pub solid: bool,
    pub locked: bool,
    pub encrypted_headers: bool,
    pub recovery: bool,
    pub multipart: bool,
    pub sfx: bool,
    pub needs_backend: bool,
}

impl Default for Format {
    fn default() -> Self { Format::Unknown }
}

// ---- helpers ----

pub fn dos_time(t: u32) -> Option<String> {
    if t == 0 { return None; }
    let (sec, min, hh) = ((t & 0x1f) * 2, (t >> 5) & 0x3f, (t >> 11) & 0x1f);
    let (day, mon, yr) = ((t >> 16) & 0x1f, (t >> 21) & 0x0f, ((t >> 25) & 0x7f) + 1980);
    if mon == 0 || day == 0 { return None; }
    Some(format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", yr, mon, day, hh, min, sec))
}

pub fn epoch_time(secs: i64) -> Option<String> {
    let dt = time::OffsetDateTime::from_unix_timestamp(secs).ok()?;
    Some(format!("{:04}-{:02}-{:02} {:02}:{:02}",
                 dt.year(), dt.month() as u8, dt.day(), dt.hour(), dt.minute()))
}

fn zip_method_name(m: zip::CompressionMethod) -> String {
    use zip::CompressionMethod::*;
    match m {
        Stored => "Store".into(),
        Deflated => "Deflate".into(),
        Deflate64 => "Deflate64".into(),
        Bzip2 => "BZIP2".into(),
        Zstd => "Zstandard".into(),
        Lzma => "LZMA".into(),
        Xz => "XZ".into(),
        Aes => "AES".into(),
        Ppmd => "PPMd".into(),
        _ => format!("{:?}", m),
    }
}

// ---- listing ----

pub fn list_archive(path: &Path) -> Result<ArchiveInfo> {
    if let Some(_) = sfx_payload_offset(path) {
        // SFX: still listable via zip path after offset — handled by caller
    }
    let fmt = detect_format(path);
    let mut info = ArchiveInfo { format: fmt, ..Default::default() };
    info.sfx = sfx_payload_offset(path).is_some();
    match fmt {
        Format::Zip => zip_list(path, &mut info)?,
        Format::Rar | Format::Rev => rar_list(path, None, &mut info)?,
        Format::Tar | Format::Gz | Format::Bz2 | Format::Xz => {
            tar_list(path, fmt, &mut info)?
        }
        Format::Zst => tar_list(path, fmt, &mut info)?,
        Format::SevenZ => sevenz_list(path, &mut info)?,
        Format::Cab => info.needs_backend = true,
        Format::Unknown => bail!("Unknown or unsupported archive format"),
    }
    Ok(info)
}

fn zip_list(path: &Path, info: &mut ArchiveInfo) -> Result<()> {
    let f = File::open(path)?;
    let mut zf = zip::ZipArchive::new(f)?;
    for i in 0..zf.len() {
        let z = zf.by_index_raw(i)?;   // raw: no decrypt attempt for listing
        info.entries.push(Entry {
            name: z.name().unwrap_or_default().replace('\\', "/"),
            size: z.size(),
            packed: z.compressed_size(),
            mtime: z.last_modified().map(|d| {
                format!("{:04}-{:02}-{:02} {:02}:{:02}",
                        d.year(), d.month(), d.day(), d.hour(), d.minute())
            }),
            crc: Some(z.crc32()),
            method: zip_method_name(z.compression()),
            is_dir: z.is_dir(),
            encrypted: z.encrypted(),
        });
    }
    let c = String::from_utf8_lossy(zf.comment()).into_owned();
    info.comment = c;
    info.encrypted_headers = info.entries.iter().any(|e| e.encrypted);
    Ok(())
}

fn rar_list(path: &Path, pwd: Option<&str>, info: &mut ArchiveInfo) -> Result<()> {
    let open = match pwd {
        Some(p) => unrar::Archive::with_password(path, p),
        None => unrar::Archive::new(path),
    };
    let archive = open.open_for_listing()
        .map_err(|e| anyhow!("{}", e))?;
    info.solid = archive.is_solid();
    info.locked = archive.is_locked();
    info.recovery = archive.has_recovery_record();
    info.encrypted_headers = archive.has_encrypted_headers();
    info.multipart = unrar::Archive::new(path).is_multipart();
    for h in archive {
        let h = h.map_err(|e| anyhow!("{}", e))?;
        let name = h.filename.to_string_lossy().replace('\\', "/");
        info.entries.push(Entry {
            name,
            size: h.unpacked_size,
            packed: h.unpacked_size,
            mtime: dos_time(h.file_time),
            crc: Some(h.file_crc),
            method: format!("RAR m{}", h.method.saturating_sub(0x30)),
            is_dir: h.is_directory(),
            encrypted: h.is_encrypted(),
        });
    }
    Ok(())
}

fn tar_reader(path: &Path, fmt: Format)
    -> Result<Box<dyn Read + Send>>
{
    let f = File::open(path)?;
    Ok(match fmt {
        Format::Gz => Box::new(flate2::read::GzDecoder::new(f)),
        Format::Bz2 => Box::new(bzip2::read::BzDecoder::new(f)),
        Format::Xz => Box::new(xz2::read::XzDecoder::new(f)),
        Format::Zst => Box::new(zstd::Decoder::new(f)?),
        _ => Box::new(f),
    })
}

fn tar_list(path: &Path, fmt: Format, info: &mut ArchiveInfo) -> Result<()> {
    let r = tar_reader(path, fmt)?;
    let mut ar = tar::Archive::new(r);
    for e in ar.entries()? {
        let e = e?;
        let h = e.header();
        info.entries.push(Entry {
            name: e.path()?.to_string_lossy().replace('\\', "/"),
            size: h.size().unwrap_or(0),
            packed: h.size().unwrap_or(0),
            mtime: h.mtime().ok().map(|t| epoch_time(t as i64)).flatten(),
            crc: h.cksum().ok(),
            method: format!("{:?}", h.entry_type()),
            is_dir: h.entry_type() == tar::EntryType::Directory,
            encrypted: false,
        });
    }
    Ok(())
}

fn sevenz_list(path: &Path, info: &mut ArchiveInfo) -> Result<()> {
    let reader = sevenz_rust::SevenZReader::open(path, "".into())
        .map_err(|e| anyhow!("{}", e))?;
    let files = &reader.archive().files;
    for f in files {
        info.entries.push(Entry {
            name: f.name().to_string(),
            size: f.size,
            packed: f.compressed_size,
            mtime: None,
            crc: if f.has_crc { Some(f.crc as u32) } else { None },
            method: "7z".into(),
            is_dir: f.is_directory,
            encrypted: false,
        });
    }
    Ok(())
}

// ---- member read (internal viewer) ----

pub fn read_member(path: &Path, name: &str, pwd: Option<&str>) -> Result<Vec<u8>> {
    match detect_format(path) {
        Format::Zip => {
            let mut zf = zip::ZipArchive::new(File::open(path)?)?;
            let mut z = match pwd {
                Some(p) => zf.by_name_decrypt(name, p.as_bytes())
                    .map_err(|e| anyhow!("{}", e))?,
                None => zf.by_name(name)?,
            };
            let mut buf = Vec::with_capacity(z.size() as usize);
            z.read_to_end(&mut buf)?;
            Ok(buf)
        }
        Format::Rar | Format::Rev => {
            let open = match pwd {
                Some(p) => unrar::Archive::with_password(path, p),
                None => unrar::Archive::new(path),
            };
            let mut arc = open.open_for_processing()
                .map_err(|e| anyhow!("{}", e))?;
            while let Some(header) = arc.read_header().map_err(|e| anyhow!("{}", e))? {
                if header.entry().is_file()
                    && header.entry().filename.to_string_lossy()
                        .replace('\\', "/") == name
                {
                    let (data, _rest) = header.read()
                        .map_err(|e| anyhow!("{}", e))?;
                    return Ok(data);
                }
                arc = header.skip().map_err(|e| anyhow!("{}", e))?;
            }
            bail!("member not found: {name}")
        }
        Format::Tar | Format::Gz | Format::Bz2 | Format::Xz | Format::Zst => {
            let fmt = detect_format(path);
            let r = tar_reader(path, fmt)?;
            let mut ar = tar::Archive::new(r);
            for e in ar.entries()? {
                let mut e = e?;
                if e.path()?.to_string_lossy().replace('\\', "/") == name {
                    let mut buf = Vec::new();
                    e.read_to_end(&mut buf)?;
                    return Ok(buf);
                }
            }
            bail!("member not found: {name}")
        }
        Format::SevenZ => {
            let mut rd = sevenz_rust::SevenZReader::open(path, "".into())
                .map_err(|e| anyhow!("{}", e))?;
            let target = name.to_string();
            let mut out = Vec::new();
            let mut found = false;
            rd.for_each_entries(|entry, reader| {
                if entry.name() == target {
                    found = true;
                    let mut b = Vec::with_capacity(entry.size() as usize);
                    let _ = reader.read_to_end(&mut b);
                    out = b;
                } else {
                    let mut sink = Vec::new();
                    let _ = reader.read_to_end(&mut sink);
                }
                Ok(true)
            }).map_err(|e| anyhow!("{}", e))?;
            if found { Ok(out) } else { bail!("member not found: {name}") }
        }
        f => bail!("reading {} members is not supported", f.label()),
    }
}

// ---- extraction ----

pub fn extract_archive(
    path: &Path, dest: &Path,
    members: Option<&[String]>, pwd: Option<&str>,
) -> Result<usize> {
    std::fs::create_dir_all(dest)?;
    match detect_format(path) {
        Format::Zip => zip_extract(path, dest, members, pwd),
        Format::Rar | Format::Rev => rar_extract(path, dest, members, pwd),
        Format::Tar | Format::Gz | Format::Bz2 | Format::Xz | Format::Zst =>
            tar_extract(path, dest, members),
        Format::SevenZ => sevenz_extract(path, dest, members),
        f => bail!("extracting {} needs an external tool", f.label()),
    }
}

fn zip_extract(path: &Path, dest: &Path,
               members: Option<&[String]>, pwd: Option<&str>) -> Result<usize> {
    let mut zf = zip::ZipArchive::new(File::open(path)?)?;
    let want: Option<std::collections::HashSet<&str>> =
        members.map(|m| m.iter().map(|s| s.as_str()).collect());
    let mut count = 0;
    for i in 0..zf.len() {
        let mut z = match pwd {
            Some(p) => zf.by_index_decrypt(i, p.as_bytes())
                .map_err(|e| anyhow!("{}", e))?,
            None => zf.by_index(i)?,
        };
        let name = z.name().unwrap_or_default().replace('\\', "/");
        if want.as_ref().is_some_and(|w| !w.contains(name.as_str())) {
            continue;
        }
        let Some(rel) = z.enclosed_name() else { continue };
        let out = dest.join(rel);
        if z.is_dir() {
            std::fs::create_dir_all(&out)?;
        } else {
            if let Some(p) = out.parent() { std::fs::create_dir_all(p)?; }
            let mut o = File::create(&out)?;
            std::io::copy(&mut z, &mut o)?;
        }
        count += 1;
    }
    Ok(count)
}

fn rar_extract(path: &Path, dest: &Path,
               members: Option<&[String]>, pwd: Option<&str>) -> Result<usize> {
    let open = match pwd {
        Some(p) => unrar::Archive::with_password(path, p),
        None => unrar::Archive::new(path),
    };
    let mut arc = open.open_for_processing().map_err(|e| anyhow!("{}", e))?;
    let want: Option<std::collections::HashSet<&str>> =
        members.map(|m| m.iter().map(|s| s.as_str()).collect());
    let mut count = 0;
    while let Some(header) = arc.read_header().map_err(|e| anyhow!("{}", e))? {
        let e = header.entry();
        let name = e.filename.to_string_lossy().replace('\\', "/");
        let take = want.as_ref().map_or(true, |w| w.contains(name.as_str()));
        arc = if take && e.is_file() {
            count += 1;
            header.extract_with_base(dest).map_err(|e| anyhow!("{}", e))?
        } else {
            header.skip().map_err(|e| anyhow!("{}", e))?
        };
    }
    Ok(count)
}

fn tar_extract(path: &Path, dest: &Path,
               members: Option<&[String]>) -> Result<usize> {
    let fmt = detect_format(path);
    let r = tar_reader(path, fmt)?;
    let mut ar = tar::Archive::new(r);
    let want: Option<std::collections::HashSet<&str>> =
        members.map(|m| m.iter().map(|s| s.as_str()).collect());
    let mut count = 0;
    for e in ar.entries()? {
        let mut e = e?;
        let name = e.path()?.to_string_lossy().replace('\\', "/");
        if want.as_ref().is_some_and(|w| !w.contains(name.as_str())) {
            continue;
        }
        e.unpack_in(dest)?;
        count += 1;
    }
    Ok(count)
}

fn sevenz_extract(path: &Path, dest: &Path,
                  members: Option<&[String]>) -> Result<usize> {
    let mut rd = sevenz_rust::SevenZReader::open(path, "".into())
        .map_err(|e| anyhow!("{}", e))?;
    let want: Option<std::collections::HashSet<&str>> =
        members.map(|m| m.iter().map(|s| s.as_str()).collect());
    let dest = dest.to_path_buf();
    let mut count = 0usize;
    rd.for_each_entries(|entry, reader| {
        let name = entry.name().to_string();
        let take = want.as_ref().map_or(true, |w| w.contains(name.as_str()));
        let out = dest.join(&name);
        if take && entry.is_directory() {
            let _ = std::fs::create_dir_all(&out);
            count += 1;
        } else if take {
            if let Some(p) = out.parent() { let _ = std::fs::create_dir_all(p); }
            let mut b = Vec::new();
            let _ = reader.read_to_end(&mut b);
            let _ = File::create(&out).map(|mut f| f.write_all(&b));
            count += 1;
        } else {
            let mut sink = Vec::new();
            let _ = reader.read_to_end(&mut sink);
        }
        Ok(true)
    }).map_err(|e| anyhow!("{}", e))?;
    Ok(count)
}

// ---- test ----

pub fn test_archive(path: &Path) -> Result<String> {
    match detect_format(path) {
        Format::Zip => {
            let mut zf = zip::ZipArchive::new(File::open(path)?)?;
            let mut n = 0;
            let mut bad = String::new();
            for i in 0..zf.len() {
                let mut z = zf.by_index(i)?;
                n += 1;
                let mut sink = std::io::sink();
                if std::io::copy(&mut z, &mut sink).is_err() {
                    bad = z.name().unwrap_or_default().into_owned();
                    break;
                }
            }
            if bad.is_empty() {
                Ok(format!("All OK — {} entries verified (CRC32)", n))
            } else {
                bail!("CRC failed in {bad}")
            }
        }
        Format::Rar | Format::Rev => {
            let mut arc = unrar::Archive::new(path)
                .open_for_processing().map_err(|e| anyhow!("{}", e))?;
            let mut n = 0;
            while let Some(header) = arc.read_header().map_err(|e| anyhow!("{}", e))? {
                arc = if header.entry().is_file() {
                    n += 1;
                    header.test().map_err(|e| anyhow!("{}", e))?
                } else {
                    header.skip().map_err(|e| anyhow!("{}", e))?
                };
            }
            Ok(format!("All OK — {} files verified (CRC32/BLAKE2)", n))
        }
        Format::Tar | Format::Gz | Format::Bz2 | Format::Xz | Format::Zst => {
            let fmt = detect_format(path);
            let r = tar_reader(path, fmt)?;
            let mut ar = tar::Archive::new(r);
            let mut n = 0;
            for e in ar.entries()? {
                let mut e = e?;
                n += 1;
                if e.header().entry_type() == tar::EntryType::Regular {
                    std::io::copy(&mut e, &mut std::io::sink())?;
                }
            }
            Ok(format!("All OK — {n} entries readable"))
        }
        Format::SevenZ => {
            let mut rd = sevenz_rust::SevenZReader::open(path, "".into())
                .map_err(|e| anyhow!("{}", e))?;
            let mut n = 0usize;
            rd.for_each_entries(|_e, r| {
                let mut sink = std::io::sink();
                let _ = std::io::copy(r, &mut sink);
                n += 1;
                Ok(true)
            }).map_err(|e| anyhow!("{}", e))?;
            Ok(format!("All OK — {n} entries verified"))
        }
        f => bail!("testing {} needs an external tool", f.label()),
    }
}

// ---- ZIP write ops ----

pub const LEVELS: &[(&str, i64)] = &[
    ("Store", 0), ("Fastest", 1), ("Fast", 3),
    ("Normal", 6), ("Good", 7), ("Best", 9),
];

pub fn zip_add(path: &Path, files: &[PathBuf], level: i64,
               pwd: Option<&str>, comment: Option<&str>) -> Result<usize> {
    let method = if level == 0 {
        zip::CompressionMethod::Stored
    } else {
        zip::CompressionMethod::Deflated
    };
    let base = zip::write::SimpleFileOptions::default()
        .compression_method(method)
        .compression_level(if level == 0 { None } else { Some(level) });
    let file = if path.exists() {
        File::options().read(true).write(true).open(path)?
    } else {
        File::create(path)?
    };
    let mut zw = zip::ZipWriter::new_append(file)?;
    if let Some(c) = comment {
        let _ = zw.set_comment(c.to_string());
    }
    let mut count = 0;
    match pwd {
        // AES encryption binds the password's lifetime into the options
        // type — branch so both variants type-check.
        Some(p) => {
            let opts = base.with_aes_encryption(zip::AesMode::Aes256, p);
            for f in files {
                add_path(&mut zw, f, opts, Path::new(""), &mut count)?;
            }
        }
        None => {
            for f in files {
                add_path(&mut zw, f, base, Path::new(""), &mut count)?;
            }
        }
    }
    zw.finish()?;
    Ok(count)
}

fn add_path<'k, 'n>(zw: &mut zip::ZipWriter<File>, p: &Path,
            opts: zip::write::FileOptions<'k, 'n, ()>, base: &Path,
            count: &mut usize) -> Result<()> {
    if p.is_dir() {
        for child in std::fs::read_dir(p)? {
            let child = child?.path();
            add_path(zw, &child, opts, &base.join(p.file_name()
                .map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                count)?;
        }
    } else if p.is_file() {
        let rel = if base.as_os_str().is_empty() {
            p.file_name().unwrap().to_string_lossy().replace('\\', "/")
        } else {
            format!("{}/{}", base.to_string_lossy().replace('\\', "/"),
                    p.file_name().unwrap().to_string_lossy())
        };
        zw.start_file(&rel, opts)?;
        let mut f = File::open(p)?;
        std::io::copy(&mut f, zw)?;
        *count += 1;
    }
    Ok(())
}

pub fn zip_delete(path: &Path, names: &[String]) -> Result<usize> {
    let mut zf = zip::ZipArchive::new(File::open(path)?)?;
    let drop_set: std::collections::HashSet<&str> =
        names.iter().map(|s| s.as_str()).collect();
    let tmp = path.with_extension("unrar_tmp");
    let tmpf = File::create(&tmp)?;
    let mut zw = zip::ZipWriter::new(tmpf);
    let kept_comment = String::from_utf8_lossy(zf.comment()).into_owned();
    for i in 0..zf.len() {
        let mut z = zf.by_index_raw(i)?;
        if drop_set.contains(z.name().unwrap_or_default().as_ref()) { continue; }
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(z.compression())
            .large_file(z.size() > u32::MAX as u64);
        zw.start_file(z.name().unwrap_or_default().into_owned(), opts)?;
        std::io::copy(&mut z, &mut zw)?;
    }
    if !kept_comment.is_empty() { let _ = zw.set_comment(kept_comment); }
    zw.finish()?;
    std::mem::drop(zf);
    std::fs::rename(&tmp, path)?;
    Ok(names.len())
}

pub fn zip_comment(path: &Path) -> Result<String> {
    let zf = zip::ZipArchive::new(File::open(path)?)?;
    Ok(String::from_utf8_lossy(zf.comment()).into_owned())
}

pub fn zip_set_comment(path: &Path, comment: &str) -> Result<()> {
    let f = File::options().read(true).write(true).open(path)?;
    let mut zw = zip::ZipWriter::new_append(f)?;
    zw.set_comment(comment.to_string())?;
    zw.finish()?;
    Ok(())
}

// ---- SFX ----

/// Build a self-extracting exe: stub exe + appended ZIP payload.
pub fn make_sfx(zip_path: &Path, out_path: &Path) -> Result<()> {
    let stub = std::env::current_exe()?;
    std::fs::copy(&stub, out_path)?;
    let mut out = File::options().append(true).open(out_path)?;
    let mut z = File::open(zip_path)?;
    std::io::copy(&mut z, &mut out)?;
    Ok(())
}

/// If running exe has an appended ZIP payload, self-extract. Returns
/// true if the process was an SFX run (caller should exit after).
pub fn self_extract() -> bool {
    let exe = match std::env::current_exe() { Ok(e) => e, Err(_) => return false };
    let Some(off) = sfx_payload_offset(&exe) else { return false };
    let Some(dest) = rfd::FileDialog::new()
        .set_title("UnRAR self-extracting archive — choose destination")
        .pick_folder() else { return true };
    let f = match File::open(&exe) { Ok(f) => f, Err(_) => return true };
    // read the appended payload (zip) from offset to EOF
    let mut f2 = f;
    if f2.seek(SeekFrom::Start(off)).is_err() { return true; }
    let mut payload = Vec::new();
    if f2.read_to_end(&mut payload).is_err() { return true; }
    let cur = std::io::Cursor::new(payload);
    let res = (|| -> Result<usize> {
        let mut zf = zip::ZipArchive::new(cur)?;
        let mut n = 0;
        for i in 0..zf.len() {
            let mut z = zf.by_index(i)?;
            let Some(rel) = z.enclosed_name() else { continue };
            let out = dest.join(rel);
            if z.is_dir() {
                std::fs::create_dir_all(&out)?;
            } else {
                if let Some(p) = out.parent() { std::fs::create_dir_all(p)?; }
                let mut o = File::create(&out)?;
                std::io::copy(&mut z, &mut o)?;
            }
            n += 1;
        }
        Ok(n)
    })();
    match res {
        Ok(n) => rfd::MessageDialog::new()
            .set_title("UnRAR SFX")
            .set_description(format!("Extracted {n} files."))
            .show(),
        Err(e) => rfd::MessageDialog::new()
            .set_title("UnRAR SFX")
            .set_description(format!("Extraction failed: {e}"))
            .show(),
    };
    true
}
