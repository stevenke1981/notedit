//! Byte buffer for MadEdit-style hex mode. Small files live in memory and support
//! overwrite / insert / delete; huge files are memory-mapped and edited by
//! overwrite patches so opening a multi-GB binary costs (almost) no RAM.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::Path;

pub const ROW: usize = 16;

enum Base { Mem(Vec<u8>), Map(memmap2::Mmap) }

#[derive(Clone, Copy, Debug, PartialEq)]
enum Op { Set { pos: u64, old: u8, new: u8 }, Ins { pos: u64, b: u8 }, Del { pos: u64, b: u8 } }

pub struct HexBuf {
    base: Base,
    patches: BTreeMap<u64, u8>,
    undo: Vec<Op>,
    redo: Vec<Op>,
}

impl HexBuf {
    pub fn from_vec(v: Vec<u8>) -> Self { HexBuf { base: Base::Mem(v), patches: BTreeMap::new(), undo: vec![], redo: vec![] } }

    /// Memory-map `path` (read-only mapping; edits are kept as patches).
    pub fn map(path: &Path) -> io::Result<Self> {
        let f = File::open(path)?;
        // SAFETY: the mapping is read-only; if another process truncates the file
        // the OS may raise SIGBUS, which is the accepted trade-off of mmap viewers.
        let m = unsafe { memmap2::Mmap::map(&f)? };
        Ok(HexBuf { base: Base::Map(m), patches: BTreeMap::new(), undo: vec![], redo: vec![] })
    }

    pub fn is_mapped(&self) -> bool { matches!(self.base, Base::Map(_)) }
    pub fn len(&self) -> u64 { match &self.base { Base::Mem(v) => v.len() as u64, Base::Map(m) => m.len() as u64 } }

    pub fn get(&self, pos: u64) -> Option<u8> {
        if let Some(&b) = self.patches.get(&pos) { return Some(b); }
        match &self.base { Base::Mem(v) => v.get(pos as usize).copied(), Base::Map(m) => m.get(pos as usize).copied() }
    }

    pub fn read(&self, start: u64, n: usize) -> Vec<u8> {
        let end = (start + n as u64).min(self.len());
        (start..end).filter_map(|p| self.get(p)).collect()
    }

    fn set_raw(&mut self, pos: u64, b: u8) {
        match &mut self.base { Base::Mem(v) => v[pos as usize] = b, Base::Map(_) => { self.patches.insert(pos, b); } }
    }

    /// Overwrite one byte (appends when `pos == len` and the buffer is in memory).
    pub fn set(&mut self, pos: u64, b: u8) -> bool {
        if pos == self.len() { return self.insert(pos, b); }
        let Some(old) = self.get(pos) else { return false };
        self.set_raw(pos, b);
        self.push(Op::Set { pos, old, new: b });
        true
    }

    pub fn insert(&mut self, pos: u64, b: u8) -> bool {
        let Base::Mem(v) = &mut self.base else { return false };
        if pos as usize > v.len() { return false; }
        v.insert(pos as usize, b);
        self.push(Op::Ins { pos, b });
        true
    }

    pub fn delete(&mut self, pos: u64) -> bool {
        let Base::Mem(v) = &mut self.base else { return false };
        if pos as usize >= v.len() { return false; }
        let b = v.remove(pos as usize);
        self.push(Op::Del { pos, b });
        true
    }

    fn push(&mut self, op: Op) { self.undo.push(op); self.redo.clear(); if self.undo.len() > 100_000 { self.undo.remove(0); } }

    fn apply(&mut self, op: Op, rev: bool) -> u64 {
        match (op, rev) {
            (Op::Set { pos, old, .. }, true) => { self.set_raw(pos, old); pos }
            (Op::Set { pos, new, .. }, false) => { self.set_raw(pos, new); pos }
            (Op::Ins { pos, .. }, true) | (Op::Del { pos, .. }, false) => { if let Base::Mem(v) = &mut self.base { v.remove(pos as usize); } pos }
            (Op::Ins { pos, b }, false) | (Op::Del { pos, b }, true) => { if let Base::Mem(v) = &mut self.base { v.insert(pos as usize, b); } pos }
        }
    }

    /// Returns the position affected.
    pub fn undo(&mut self) -> Option<u64> { let op = self.undo.pop()?; let p = self.apply(op, true); self.redo.push(op); Some(p) }
    pub fn redo(&mut self) -> Option<u64> { let op = self.redo.pop()?; let p = self.apply(op, false); self.undo.push(op); Some(p) }

    /// Whole contents (for switching back to text mode); `None` when mapped and larger than `limit`.
    pub fn to_vec(&self, limit: u64) -> Option<Vec<u8>> {
        if self.len() > limit { return None; }
        Some(self.read(0, self.len() as usize))
    }

    /// Stream the contents (with patches applied) into `w`.
    pub fn write_to(&self, w: &mut impl Write) -> io::Result<()> {
        let mut w = BufWriter::with_capacity(1 << 20, w);
        match &self.base {
            Base::Mem(v) => w.write_all(v)?,
            Base::Map(m) => {
                let mut last = 0usize;
                for (&p, &b) in &self.patches {
                    let p = p as usize;
                    w.write_all(&m[last..p])?;
                    w.write_all(&[b])?;
                    last = p + 1;
                }
                w.write_all(&m[last..])?;
            }
        }
        w.flush()
    }

    /// Save to `path`. For mapped buffers the mapping is released before the file is
    /// replaced (required on Windows) and re-established afterwards.
    pub fn save(&mut self, path: &Path) -> io::Result<()> {
        if self.is_mapped() {
            let tmp = path.with_extension(format!("notedit-hex-{}", std::process::id()));
            { let mut f = File::create(&tmp)?; self.write_to(&mut f)?; f.sync_all()?; }
            self.base = Base::Mem(Vec::new());
            let r = std::fs::rename(&tmp, path);
            if r.is_err() { let _ = std::fs::remove_file(&tmp); }
            let m = HexBuf::map(path)?;
            self.base = m.base;
            self.patches.clear();
            self.undo.clear(); self.redo.clear();
            r
        } else {
            crate::file_io::durable_write_with(path, |f| self.write_to(f))
        }
    }
}

/// `00000010` style offset label.
pub fn offset_label(off: u64, wide: bool) -> String { if wide { format!("{off:010X}") } else { format!("{off:08X}") } }

/// Printable ASCII representation of a byte for the right-hand column.
pub fn ascii(b: u8) -> char { if (0x20..0x7f).contains(&b) { b as char } else { '.' } }

/// Heuristic: does the data look binary (NUL bytes or many control chars)?
pub fn looks_binary(sample: &[u8]) -> bool {
    if sample.starts_with(&[0xFF, 0xFE]) || sample.starts_with(&[0xFE, 0xFF]) { return false; }
    if sample.contains(&0) { return true; }
    let ctrl = sample.iter().filter(|&&b| b < 0x09 || (0x0e..0x20).contains(&b)).count();
    ctrl * 20 > sample.len().max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mem_edit_undo() {
        let mut h = HexBuf::from_vec(b"abc".to_vec());
        assert!(h.set(1, b'X'));
        assert!(h.insert(0, b'0'));
        assert!(h.delete(3));
        assert!(h.set(3, b'!')); // append
        assert_eq!(h.read(0, 10), b"0aX!");
        h.undo(); h.undo(); h.undo(); h.undo();
        assert_eq!(h.read(0, 10), b"abc");
        h.redo();
        assert_eq!(h.read(0, 10), b"aXc");
    }
    #[test]
    fn mapped_patches_and_save() {
        let dir = std::env::temp_dir().join(format!("notedit-hex-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("a.bin");
        std::fs::write(&p, [0u8, 1, 2, 3, 4]).unwrap();
        let mut h = HexBuf::map(&p).unwrap();
        assert!(h.is_mapped());
        assert!(!h.insert(0, 9));
        assert!(h.set(2, 0xAA));
        assert_eq!(h.read(0, 5), [0, 1, 0xAA, 3, 4]);
        h.save(&p).unwrap();
        assert_eq!(std::fs::read(&p).unwrap(), [0, 1, 0xAA, 3, 4]);
        assert!(h.is_mapped());
        std::fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn binary_detection() {
        assert!(looks_binary(b"\x7fELF\x02\x01\x00\x00"));
        assert!(!looks_binary("hello 中文\r\n".as_bytes()));
    }
}
