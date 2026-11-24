//! Dante's filesystem.
//!
//! Layout, in 4 KiB blocks:
//!   0                superblock
//!   1..              block bitmap (one bit per block, 1 = in use)
//!   ..               inode table (128-byte inodes, 32 per block)
//!   data_start..     file and folder contents
//!
//! An inode holds a file's size, its modified time and the blocks holding its
//! contents: 12 direct blocks plus one block of further block numbers, so a
//! file can be a little over 4 MiB. A folder is a file of 64-byte entries.
//! Inode 1 is the root folder.

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use crate::disk::{DISK, SECTOR};
use crate::time;

const BS: usize = 4096;
const SECTORS_PER_BLOCK: u64 = (BS / SECTOR) as u64;
const MAGIC: &[u8; 8] = b"DANTEFS1";
const INODE_SIZE: usize = 128;
const INODES_PER_BLOCK: usize = BS / INODE_SIZE;
const DIRECT: usize = 12;
const PER_INDIRECT: usize = BS / 4;
pub const MAX_FILE: usize = (DIRECT + PER_INDIRECT) * BS;
const ENTRY_SIZE: usize = 64;
const NAME_MAX: usize = 56;
const ROOT: u32 = 1;

const FREE: u8 = 0;
const FILE: u8 = 1;
const DIR: u8 = 2;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Error {
    NoDisk,
    NotFound,
    Exists,
    NotDir,
    IsDir,
    NotEmpty,
    NoSpace,
    TooBig,
    BadName,
    Io,
}

impl Error {
    pub fn text(self) -> &'static str {
        match self {
            Error::NoDisk => "no disk",
            Error::NotFound => "no such file or directory",
            Error::Exists => "already exists",
            Error::NotDir => "not a directory",
            Error::IsDir => "is a directory",
            Error::NotEmpty => "directory not empty",
            Error::NoSpace => "disk full",
            Error::TooBig => "file too large",
            Error::BadName => "invalid name",
            Error::Io => "disk error",
        }
    }
}

pub type Result<T> = core::result::Result<T, Error>;

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u32,
    pub modified: u64,
}

#[derive(Clone, Copy, Default)]
struct Inode {
    kind: u8,
    size: u32,
    modified: u64,
    direct: [u32; DIRECT],
    indirect: u32,
}

impl Inode {
    fn decode(b: &[u8]) -> Inode {
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
        let mut direct = [0; DIRECT];
        for (i, d) in direct.iter_mut().enumerate() {
            *d = u32_at(16 + 4 * i);
        }
        Inode {
            kind: b[0],
            size: u32_at(4),
            modified: u64::from_le_bytes(b[8..16].try_into().unwrap()),
            direct,
            indirect: u32_at(64),
        }
    }

    fn encode(&self, b: &mut [u8]) {
        b.fill(0);
        b[0] = self.kind;
        b[4..8].copy_from_slice(&self.size.to_le_bytes());
        b[8..16].copy_from_slice(&self.modified.to_le_bytes());
        for (i, d) in self.direct.iter().enumerate() {
            b[16 + 4 * i..20 + 4 * i].copy_from_slice(&d.to_le_bytes());
        }
        b[64..68].copy_from_slice(&self.indirect.to_le_bytes());
    }
}

struct Fs {
    blocks: u32,
    bitmap_start: u32,
    bitmap: Vec<u8>,
    inode_start: u32,
    inodes: Vec<Inode>,
    data_start: u32,
}

static FS: Mutex<Option<Fs>> = Mutex::new(None);

fn read_block(n: u32, buf: &mut [u8]) -> Result<()> {
    let mut d = DISK.lock();
    let d = d.as_mut().ok_or(Error::NoDisk)?;
    if d.read(n as u64 * SECTORS_PER_BLOCK, buf) { Ok(()) } else { Err(Error::Io) }
}

fn write_block(n: u32, buf: &[u8]) -> Result<()> {
    let mut d = DISK.lock();
    let d = d.as_mut().ok_or(Error::NoDisk)?;
    if d.write(n as u64 * SECTORS_PER_BLOCK, buf) { Ok(()) } else { Err(Error::Io) }
}

fn now() -> u64 {
    time::now().timestamp()
}

/// Mount the disk's filesystem, formatting it first if it has none.
pub fn init() {
    let sectors = match DISK.lock().as_ref() {
        Some(d) => d.sectors,
        None => return,
    };
    let mut sb = vec![0u8; BS];
    if read_block(0, &mut sb).is_err() {
        crate::log!("fs: could not read the disk");
        return;
    }
    if &sb[0..8] != MAGIC {
        let blocks = (sectors / SECTORS_PER_BLOCK) as u32;
        crate::log!("fs: formatting {} MiB", blocks as u64 * BS as u64 / (1024 * 1024));
        if format(blocks).is_err() {
            crate::log!("fs: format failed");
            return;
        }
        read_block(0, &mut sb).ok();
    }
    match mount(&sb) {
        Ok(fs) => {
            let used = fs.bitmap_used();
            crate::log!("fs: mounted, {} of {} blocks used", used, fs.blocks);
            *FS.lock() = Some(fs);
        }
        Err(e) => crate::log!("fs: mount failed: {}", e.text()),
    }
}

fn format(blocks: u32) -> Result<()> {
    let bitmap_blocks = (blocks as usize).div_ceil(BS * 8) as u32;
    let inode_count = ((blocks / 16).max(256) as usize).div_ceil(INODES_PER_BLOCK) * INODES_PER_BLOCK;
    let inode_blocks = (inode_count / INODES_PER_BLOCK) as u32;
    let inode_start = 1 + bitmap_blocks;
    let data_start = inode_start + inode_blocks;

    let mut sb = vec![0u8; BS];
    sb[0..8].copy_from_slice(MAGIC);
    for (i, v) in [1u32, blocks, 1, bitmap_blocks, inode_start, inode_blocks, inode_count as u32, data_start].iter().enumerate() {
        sb[8 + 4 * i..12 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }

    let zero = vec![0u8; BS];
    for b in 1..data_start {
        write_block(b, &zero)?;
    }
    let mut fs = Fs {
        blocks,
        bitmap_start: 1,
        bitmap: vec![0u8; bitmap_blocks as usize * BS],
        inode_start,
        inodes: vec![Inode::default(); inode_count],
        data_start,
    };
    for b in 0..data_start {
        fs.set_used(b, true);
    }
    fs.inodes[ROOT as usize] = Inode { kind: DIR, modified: now(), ..Default::default() };
    fs.save_inode(ROOT)?;
    fs.save_bitmap()?;
    write_block(0, &sb)?;
    fs.mkdir("/Documents")?;
    fs.mkdir("/Notes")?;
    Ok(())
}

fn mount(sb: &[u8]) -> Result<Fs> {
    let at = |i: usize| u32::from_le_bytes(sb[8 + 4 * i..12 + 4 * i].try_into().unwrap());
    let (blocks, bitmap_start, bitmap_blocks, inode_start, inode_blocks, inode_count, data_start) =
        (at(1), at(2), at(3), at(4), at(5), at(6), at(7));
    let mut bitmap = vec![0u8; bitmap_blocks as usize * BS];
    for i in 0..bitmap_blocks as usize {
        read_block(bitmap_start + i as u32, &mut bitmap[i * BS..(i + 1) * BS])?;
    }
    let mut inodes = Vec::with_capacity(inode_count as usize);
    let mut buf = vec![0u8; BS];
    for b in 0..inode_blocks {
        read_block(inode_start + b, &mut buf)?;
        for k in 0..INODES_PER_BLOCK {
            inodes.push(Inode::decode(&buf[k * INODE_SIZE..(k + 1) * INODE_SIZE]));
        }
    }
    inodes.truncate(inode_count as usize);
    Ok(Fs { blocks, bitmap_start, bitmap, inode_start, inodes, data_start })
}

impl Fs {
    // ---- Blocks ----

    fn is_used(&self, b: u32) -> bool {
        self.bitmap[b as usize / 8] & (1 << (b % 8)) != 0
    }

    fn set_used(&mut self, b: u32, used: bool) {
        if used {
            self.bitmap[b as usize / 8] |= 1 << (b % 8);
        } else {
            self.bitmap[b as usize / 8] &= !(1 << (b % 8));
        }
    }

    fn bitmap_used(&self) -> u32 {
        (0..self.blocks).filter(|&b| self.is_used(b)).count() as u32
    }

    fn save_bitmap(&self) -> Result<()> {
        for (i, chunk) in self.bitmap.chunks(BS).enumerate() {
            write_block(self.bitmap_start + i as u32, chunk)?;
        }
        Ok(())
    }

    fn alloc_block(&mut self) -> Result<u32> {
        let b = (self.data_start..self.blocks).find(|&b| !self.is_used(b)).ok_or(Error::NoSpace)?;
        self.set_used(b, true);
        Ok(b)
    }

    fn free_blocks(&self) -> u32 {
        self.blocks - self.bitmap_used()
    }

    // ---- Inodes ----

    fn save_inode(&self, ino: u32) -> Result<()> {
        let first = ino as usize / INODES_PER_BLOCK * INODES_PER_BLOCK;
        let mut buf = vec![0u8; BS];
        for k in 0..INODES_PER_BLOCK {
            if let Some(n) = self.inodes.get(first + k) {
                n.encode(&mut buf[k * INODE_SIZE..(k + 1) * INODE_SIZE]);
            }
        }
        write_block(self.inode_start + (ino as usize / INODES_PER_BLOCK) as u32, &buf)
    }

    fn alloc_inode(&mut self, kind: u8) -> Result<u32> {
        let ino = (ROOT as usize + 1..self.inodes.len()).find(|&i| self.inodes[i].kind == FREE).ok_or(Error::NoSpace)? as u32;
        self.inodes[ino as usize] = Inode { kind, modified: now(), ..Default::default() };
        Ok(ino)
    }

    /// The disk blocks holding an inode's contents, in order.
    fn blocks_of(&self, n: &Inode) -> Result<Vec<u32>> {
        let count = (n.size as usize).div_ceil(BS);
        let mut list: Vec<u32> = n.direct.iter().copied().take(count.min(DIRECT)).collect();
        if count > DIRECT {
            let mut buf = vec![0u8; BS];
            read_block(n.indirect, &mut buf)?;
            for i in 0..count - DIRECT {
                list.push(u32::from_le_bytes(buf[4 * i..4 * i + 4].try_into().unwrap()));
            }
        }
        Ok(list)
    }

    fn read_data(&self, ino: u32) -> Result<Vec<u8>> {
        let n = self.inodes[ino as usize];
        let mut out = vec![0u8; n.size as usize];
        let mut buf = vec![0u8; BS];
        for (i, b) in self.blocks_of(&n)?.into_iter().enumerate() {
            read_block(b, &mut buf)?;
            let start = i * BS;
            let end = (start + BS).min(out.len());
            out[start..end].copy_from_slice(&buf[..end - start]);
        }
        Ok(out)
    }

    fn release(&mut self, ino: u32) -> Result<()> {
        let n = self.inodes[ino as usize];
        for b in self.blocks_of(&n)? {
            self.set_used(b, false);
        }
        if (n.size as usize).div_ceil(BS) > DIRECT {
            self.set_used(n.indirect, false);
        }
        Ok(())
    }

    /// Replace an inode's contents.
    fn write_data(&mut self, ino: u32, data: &[u8]) -> Result<()> {
        if data.len() > MAX_FILE {
            return Err(Error::TooBig);
        }
        let needed = data.len().div_ceil(BS) + if data.len().div_ceil(BS) > DIRECT { 1 } else { 0 };
        let old = self.inodes[ino as usize];
        let old_count = (old.size as usize).div_ceil(BS);
        let reusable = old_count + if old_count > DIRECT { 1 } else { 0 };
        if needed > self.free_blocks() as usize + reusable {
            return Err(Error::NoSpace);
        }
        self.release(ino)?;

        let count = data.len().div_ceil(BS);
        let mut n = Inode { kind: old.kind, size: data.len() as u32, modified: now(), ..Default::default() };
        let mut indirect = vec![0u8; BS];
        let mut buf = vec![0u8; BS];
        for i in 0..count {
            let b = self.alloc_block()?;
            let chunk = &data[i * BS..((i + 1) * BS).min(data.len())];
            buf.fill(0);
            buf[..chunk.len()].copy_from_slice(chunk);
            write_block(b, &buf)?;
            if i < DIRECT {
                n.direct[i] = b;
            } else {
                indirect[4 * (i - DIRECT)..4 * (i - DIRECT) + 4].copy_from_slice(&b.to_le_bytes());
            }
        }
        if count > DIRECT {
            n.indirect = self.alloc_block()?;
            write_block(n.indirect, &indirect)?;
        }
        self.inodes[ino as usize] = n;
        self.save_inode(ino)?;
        self.save_bitmap()
    }

    // ---- Folders ----

    fn entries(&self, dir: u32) -> Result<Vec<(String, u32)>> {
        if self.inodes[dir as usize].kind != DIR {
            return Err(Error::NotDir);
        }
        let data = self.read_data(dir)?;
        Ok(data
            .chunks(ENTRY_SIZE)
            .filter_map(|e| {
                let ino = u32::from_le_bytes(e[0..4].try_into().unwrap());
                let len = e[5] as usize;
                (ino != 0 && len <= NAME_MAX).then(|| (String::from_utf8_lossy(&e[8..8 + len]).to_string(), ino))
            })
            .collect())
    }

    fn save_entries(&mut self, dir: u32, entries: &[(String, u32)]) -> Result<()> {
        let mut data = vec![0u8; entries.len() * ENTRY_SIZE];
        for (i, (name, ino)) in entries.iter().enumerate() {
            let e = &mut data[i * ENTRY_SIZE..(i + 1) * ENTRY_SIZE];
            e[0..4].copy_from_slice(&ino.to_le_bytes());
            e[4] = self.inodes[*ino as usize].kind;
            e[5] = name.len() as u8;
            e[8..8 + name.len()].copy_from_slice(name.as_bytes());
        }
        self.write_data(dir, &data)
    }

    fn lookup(&self, path: &str) -> Result<u32> {
        let mut ino = ROOT;
        for part in path.split('/').filter(|p| !p.is_empty()) {
            ino = self.entries(ino)?.into_iter().find(|(n, _)| n == part).map(|(_, i)| i).ok_or(Error::NotFound)?;
        }
        Ok(ino)
    }

    /// The parent folder's inode and the final name in `path`.
    fn parent<'a>(&self, path: &'a str) -> Result<(u32, &'a str)> {
        let path = path.trim_end_matches('/');
        let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
        if name.is_empty() || name.len() > NAME_MAX || name == "." || name == ".." {
            return Err(Error::BadName);
        }
        Ok((self.lookup(dir)?, name))
    }

    fn create(&mut self, path: &str, kind: u8) -> Result<u32> {
        let (dir, name) = self.parent(path)?;
        let mut list = self.entries(dir)?;
        if list.iter().any(|(n, _)| n == name) {
            return Err(Error::Exists);
        }
        let ino = self.alloc_inode(kind)?;
        self.save_inode(ino)?;
        list.push((name.to_string(), ino));
        self.save_entries(dir, &list)?;
        Ok(ino)
    }

    fn mkdir(&mut self, path: &str) -> Result<()> {
        self.create(path, DIR).map(|_| ())
    }

    fn entry(&self, name: String, ino: u32) -> Entry {
        let n = &self.inodes[ino as usize];
        Entry { name, dir: n.kind == DIR, size: n.size, modified: n.modified }
    }
}

// ---- The public interface: whole files at a time, by absolute path ----

fn with<T>(f: impl FnOnce(&mut Fs) -> Result<T>) -> Result<T> {
    let mut fs = FS.lock();
    f(fs.as_mut().ok_or(Error::NoDisk)?)
}

pub fn read(path: &str) -> Result<Vec<u8>> {
    with(|fs| {
        let ino = fs.lookup(path)?;
        if fs.inodes[ino as usize].kind == DIR {
            return Err(Error::IsDir);
        }
        fs.read_data(ino)
    })
}

/// Create or replace a file.
pub fn write(path: &str, data: &[u8]) -> Result<()> {
    with(|fs| {
        let ino = match fs.lookup(path) {
            Ok(i) => i,
            Err(Error::NotFound) => fs.create(path, FILE)?,
            Err(e) => return Err(e),
        };
        if fs.inodes[ino as usize].kind == DIR {
            return Err(Error::IsDir);
        }
        fs.write_data(ino, data)
    })
}

pub fn mkdir(path: &str) -> Result<()> {
    with(|fs| fs.mkdir(path))
}

/// Folders first, then files, each sorted by name.
pub fn list(path: &str) -> Result<Vec<Entry>> {
    with(|fs| {
        let dir = fs.lookup(path)?;
        let mut out: Vec<Entry> = fs.entries(dir)?.into_iter().map(|(n, i)| fs.entry(n, i)).collect();
        out.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
        Ok(out)
    })
}

pub fn stat(path: &str) -> Result<Entry> {
    with(|fs| {
        let ino = fs.lookup(path)?;
        let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or("").to_string();
        Ok(fs.entry(if name.is_empty() { "/".into() } else { name }, ino))
    })
}

/// Remove a file or an empty folder.
pub fn remove(path: &str) -> Result<()> {
    with(|fs| {
        let (dir, name) = fs.parent(path)?;
        let mut list = fs.entries(dir)?;
        let pos = list.iter().position(|(n, _)| n == name).ok_or(Error::NotFound)?;
        let ino = list[pos].1;
        if fs.inodes[ino as usize].kind == DIR && !fs.entries(ino)?.is_empty() {
            return Err(Error::NotEmpty);
        }
        fs.release(ino)?;
        fs.inodes[ino as usize] = Inode::default();
        fs.save_inode(ino)?;
        list.remove(pos);
        fs.save_entries(dir, &list)
    })
}

/// Move or rename. If `to` is an existing folder, the item moves into it.
pub fn rename(from: &str, to: &str) -> Result<()> {
    with(|fs| {
        let (src_dir, src_name) = fs.parent(from)?;
        let src_name = src_name.to_string();
        let ino = fs.lookup(from)?;
        let target = match fs.lookup(to) {
            Ok(t) if fs.inodes[t as usize].kind == DIR => alloc::format!("{}/{}", to.trim_end_matches('/'), src_name),
            Ok(_) => return Err(Error::Exists),
            Err(Error::NotFound) => to.to_string(),
            Err(e) => return Err(e),
        };
        // A folder can't move inside itself.
        let from_slash = alloc::format!("{}/", from.trim_end_matches('/'));
        if fs.inodes[ino as usize].kind == DIR && target.starts_with(&from_slash) {
            return Err(Error::BadName);
        }
        let (dst_dir, dst_name) = fs.parent(&target)?;
        let dst_name = dst_name.to_string();
        let mut dst = fs.entries(dst_dir)?;
        if dst.iter().any(|(n, _)| *n == dst_name) {
            return Err(Error::Exists);
        }
        let mut src = fs.entries(src_dir)?;
        src.retain(|(n, _)| *n != src_name);
        if src_dir == dst_dir {
            src.push((dst_name, ino));
            return fs.save_entries(src_dir, &src);
        }
        fs.save_entries(src_dir, &src)?;
        dst.push((dst_name, ino));
        fs.save_entries(dst_dir, &dst)
    })
}

/// (used, total) bytes.
pub fn usage() -> Option<(u64, u64)> {
    let fs = FS.lock();
    let fs = fs.as_ref()?;
    Some((fs.bitmap_used() as u64 * BS as u64, fs.blocks as u64 * BS as u64))
}

/// Files and folders whose names contain `query`, as (path, is folder).
pub fn find(query: &str, limit: usize) -> Vec<(String, bool)> {
    let q = query.to_lowercase();
    let mut found = Vec::new();
    let mut queue = vec![String::from("")];
    while let Some(dir) = queue.pop() {
        let Ok(entries) = list(if dir.is_empty() { "/" } else { &dir }) else { continue };
        for e in entries {
            let path = alloc::format!("{dir}/{}", e.name);
            if e.name.to_lowercase().contains(&q) {
                found.push((path.clone(), e.dir));
                if found.len() >= limit {
                    return found;
                }
            }
            if e.dir {
                queue.push(path);
            }
        }
    }
    found
}

/// Resolve `path` against the folder `cwd`, handling "." and "..".
pub fn resolve(cwd: &str, path: &str) -> String {
    let mut parts: Vec<&str> = if path.starts_with('/') { Vec::new() } else { cwd.split('/').filter(|p| !p.is_empty()).collect() };
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    let mut out = String::new();
    for p in parts {
        out.push('/');
        out.push_str(p);
    }
    if out.is_empty() { String::from("/") } else { out }
}

/// "1.2 KB" style sizes.
pub fn size_text(bytes: u64) -> String {
    match bytes {
        b if b < 1024 => alloc::format!("{b} B"),
        b if b < 1024 * 1024 => alloc::format!("{:.1} KB", b as f32 / 1024.0),
        b => alloc::format!("{:.1} MB", b as f32 / (1024.0 * 1024.0)),
    }
}
