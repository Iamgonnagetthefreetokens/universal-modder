//! File-backed, double-buffered regions (docs/PROTOCOL.md §2).
//!
//! One writer, one reader, no locks. The writer finishes a payload in one slot and then commits with
//! a single small write of the header's commit fields; the reader takes the header as the truth and
//! verifies with a re-read plus a CRC. A region is validated before it is used, and anything that
//! fails validation is reported as an error the caller can ignore for a frame — never a panic.

use crate::crc32::crc32;
use crate::{BridgeError, Result, MAGIC, PROTOCOL_VERSION};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Header size in bytes.
pub const HEADER_SIZE: usize = 64;
/// Payload slots (double buffering).
pub const SLOTS: usize = 2;
/// Offset of the `seq` field; the commit is the 28 bytes from here.
pub const COMMIT_OFFSET: u64 = 16;
/// Length of the commit record.
pub const COMMIT_LEN: usize = 28;
/// Header flag: the writer is alive.
pub const FLAG_WRITER_ALIVE: u16 = 1 << 0;
/// Header flag: the writer asks the reader to reset.
pub const FLAG_RESET: u16 = 1 << 1;

/// A committed payload and how old it is.
#[derive(Debug, Clone)]
pub struct ReadResult {
    /// The payload bytes.
    pub payload: Vec<u8>,
    /// Commit sequence (starts at 1).
    pub seq: u64,
    /// The writer's timestamp for this commit, microseconds.
    pub write_ts_us: u64,
    /// How long ago the writer committed, milliseconds (using this process's clock).
    pub age_ms: f64,
}

/// A region file. Either a writer ([`Region::create`]) or a reader ([`Region::open`]).
#[derive(Debug)]
pub struct Region {
    file: File,
    path: PathBuf,
    kind: u16,
    payload_size: usize,
    writer: bool,
    seq: u64,
    slot: usize,
    torn: u64,
}

impl Region {
    /// Create (or truncate) a region and publish an empty header, claiming the writer bit.
    pub fn create(path: &Path, kind: u16, payload_size: usize) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)?;
        let mut header = [0u8; HEADER_SIZE];
        header[0..4].copy_from_slice(&MAGIC);
        header[4..6].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
        header[6..8].copy_from_slice(&kind.to_le_bytes());
        header[8..12].copy_from_slice(&(payload_size as u32).to_le_bytes());
        header[12..14].copy_from_slice(&(SLOTS as u16).to_le_bytes());
        header[14..16].copy_from_slice(&(HEADER_SIZE as u16).to_le_bytes());
        // seq = 0, slot = 0, flags = writer alive, payload_len = 0, crc = 0
        header[26..28].copy_from_slice(&FLAG_WRITER_ALIVE.to_le_bytes());
        header[36..44].copy_from_slice(&crate::now_us().to_le_bytes());
        header[44..48].copy_from_slice(&std::process::id().to_le_bytes());
        file.write_all(&header)?;
        file.set_len((HEADER_SIZE + SLOTS * payload_size) as u64)?;
        file.flush()?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            kind,
            payload_size,
            writer: true,
            seq: 0,
            slot: 0,
            torn: 0,
        })
    }

    /// Open a region for reading. Fails loudly on a magic/version/kind mismatch.
    pub fn open(path: &Path, kind: u16) -> Result<Self> {
        let mut file = OpenOptions::new().read(true).open(path)?;
        let mut header = [0u8; HEADER_SIZE];
        file.read_exact(&mut header)?;
        if header[0..4] != MAGIC {
            return Err(BridgeError::BadMagic);
        }
        let version = u16::from_le_bytes([header[4], header[5]]);
        if version != PROTOCOL_VERSION {
            return Err(BridgeError::BadVersion(version));
        }
        let file_kind = u16::from_le_bytes([header[6], header[7]]);
        if file_kind != kind {
            return Err(BridgeError::BadKind(file_kind));
        }
        let payload_size = u32::from_le_bytes([header[8], header[9], header[10], header[11]]) as usize;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            kind,
            payload_size,
            writer: false,
            seq: 0,
            slot: 0,
            torn: 0,
        })
    }

    /// The path this region lives at.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Payload slot size in bytes.
    pub fn payload_size(&self) -> usize {
        self.payload_size
    }

    /// Reads that needed a retry because the writer committed mid-read.
    pub fn torn_reads(&self) -> u64 {
        self.torn
    }

    /// Write and commit a payload. Returns the new sequence number.
    pub fn write(&mut self, payload: &[u8]) -> Result<u64> {
        if !self.writer {
            return Err(BridgeError::Malformed("region is not open for writing"));
        }
        if payload.len() > self.payload_size {
            return Err(BridgeError::TooLarge("payload larger than the region slot"));
        }
        let offset = (HEADER_SIZE + self.slot * self.payload_size) as u64;
        write_at(&mut self.file, payload, offset)?;
        self.seq += 1;
        let crc = crc32(payload);
        let ts = crate::now_us();
        let mut commit = [0u8; COMMIT_LEN];
        commit[0..8].copy_from_slice(&self.seq.to_le_bytes());
        commit[8..10].copy_from_slice(&(self.slot as u16).to_le_bytes());
        commit[10..12].copy_from_slice(&FLAG_WRITER_ALIVE.to_le_bytes());
        commit[12..16].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        commit[16..20].copy_from_slice(&crc.to_le_bytes());
        commit[20..28].copy_from_slice(&ts.to_le_bytes());
        write_at(&mut self.file, &commit, COMMIT_OFFSET)?;
        self.slot ^= 1;
        Ok(self.seq)
    }

    /// Read the committed payload, verifying that it did not change mid-read.
    ///
    /// `Ok(None)` means "nothing committed yet"; `Err(BridgeError::Torn)` means the writer kept
    /// committing through every retry (call again next frame).
    pub fn read(&mut self) -> Result<Option<ReadResult>> {
        let mut header = [0u8; HEADER_SIZE];
        read_at(&mut self.file, &mut header, 0)?;
        let seq0 = u64::from_le_bytes(header[16..24].try_into().unwrap_or([0; 8]));
        if seq0 == 0 {
            return Ok(None);
        }
        for _ in 0..8 {
            let mut commit = [0u8; COMMIT_LEN];
            read_at(&mut self.file, &mut commit, COMMIT_OFFSET)?;
            let seq = u64::from_le_bytes(commit[0..8].try_into().unwrap_or([0; 8]));
            let slot = u16::from_le_bytes(commit[8..10].try_into().unwrap_or([0; 2])) as usize;
            let len = u32::from_le_bytes(commit[12..16].try_into().unwrap_or([0; 4])) as usize;
            let crc = u32::from_le_bytes(commit[16..20].try_into().unwrap_or([0; 4]));
            let ts = u64::from_le_bytes(commit[20..28].try_into().unwrap_or([0; 8]));
            if len > self.payload_size || slot >= SLOTS {
                return Err(BridgeError::TooLarge("committed payload length"));
            }
            let mut payload = vec![0u8; len];
            read_at(
                &mut self.file,
                &mut payload,
                (HEADER_SIZE + slot * self.payload_size) as u64,
            )?;
            let mut again = [0u8; COMMIT_LEN];
            read_at(&mut self.file, &mut again, COMMIT_OFFSET)?;
            if again != commit {
                self.torn += 1;
                continue; // the writer committed while we were reading: try again
            }
            if crc32(&payload) != crc {
                self.torn += 1;
                continue; // caught a partial write: try again
            }
            return Ok(Some(ReadResult {
                payload,
                seq,
                write_ts_us: ts,
                age_ms: (crate::now_us().saturating_sub(ts)) as f64 / 1000.0,
            }));
        }
        Err(BridgeError::Torn)
    }

    /// True if the writer has not cleared its alive bit (set on create, cleared by `close_clean`).
    pub fn writer_alive(&mut self) -> bool {
        let mut flags = [0u8; 2];
        if read_at(&mut self.file, &mut flags, 26).is_err() {
            return false;
        }
        u16::from_le_bytes(flags) & FLAG_WRITER_ALIVE != 0
    }

    /// Clear the writer bit. Call this on a clean shutdown so the peer can say "offline", not "lost".
    pub fn close_clean(&mut self) {
        if !self.writer {
            return;
        }
        let mut flags = [0u8; 2];
        if read_at(&mut self.file, &mut flags, 26).is_ok() {
            let cleared = u16::from_le_bytes(flags) & !FLAG_WRITER_ALIVE;
            let _ = write_at(&mut self.file, &cleared.to_le_bytes(), 26);
        }
    }

    /// Region kind (1 state, 2 terrain, 3 frame).
    pub fn kind(&self) -> u16 {
        self.kind
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        self.close_clean();
    }
}

// ---------------------------------------------------------------- positioned I/O
//
// std's positioned read/write lives behind per-platform extension traits. Both games ship on Windows
// and Linux (RecWars also on wasm, where the bridge is compiled out), so both arms are needed.

#[cfg(unix)]
fn read_at(file: &mut File, buf: &mut [u8], offset: u64) -> Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(buf, offset)?;
    Ok(())
}

#[cfg(unix)]
fn write_at(file: &mut File, buf: &[u8], offset: u64) -> Result<()> {
    use std::os::unix::fs::FileExt;
    file.write_all_at(buf, offset)?;
    Ok(())
}

#[cfg(windows)]
fn read_at(file: &mut File, buf: &mut [u8], offset: u64) -> Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0usize;
    while done < buf.len() {
        let n = file.seek_read(&mut buf[done..], offset + done as u64)?;
        if n == 0 {
            return Err(BridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "short read",
            )));
        }
        done += n;
    }
    Ok(())
}

#[cfg(windows)]
fn write_at(file: &mut File, buf: &[u8], offset: u64) -> Result<()> {
    use std::os::windows::fs::FileExt;
    let mut done = 0usize;
    while done < buf.len() {
        let n = file.seek_write(&buf[done..], offset + done as u64)?;
        if n == 0 {
            return Err(BridgeError::Io(std::io::Error::new(
                std::io::ErrorKind::WriteZero,
                "short write",
            )));
        }
        done += n;
    }
    Ok(())
}

/// Fallback for platforms without positioned I/O (wasm has neither, and neither does the bridge).
#[cfg(not(any(unix, windows)))]
#[allow(unused_imports)]
use std::io::{Seek, SeekFrom};

/// Fallback for platforms without positioned I/O (wasm has neither, and neither does the bridge).
#[cfg(not(any(unix, windows)))]
fn read_at(file: &mut File, buf: &mut [u8], offset: u64) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)?;
    Ok(())
}

/// Fallback for platforms without positioned I/O.
#[cfg(not(any(unix, windows)))]
fn write_at(file: &mut File, buf: &[u8], offset: u64) -> Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.write_all(buf)?;
    Ok(())
}
