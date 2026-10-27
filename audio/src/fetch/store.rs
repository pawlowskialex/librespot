//! Where a streaming file's downloaded bytes are kept until they are read: the whole file in a
//! temporary file (the default), or, with `AudioFetchParams::memory_limit`, at most that much of it
//! in memory, in blocks of `BLOCK` bytes. When a new block would go over the limit, the block
//! farthest behind the reader is dropped first (and if none is behind it, the one farthest ahead).
//! The caller removes what was dropped from its downloaded and requested sets, so a seek back
//! downloads it again.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

use tempfile::NamedTempFile;

use crate::range_set::Range;

pub(super) const BLOCK: usize = 32 * 1024;

const STORE_POISON_MSG: &str = "audio file store mutex should not be poisoned";

pub(super) struct Store {
    file_size: usize,
    backing: Backing,
}

enum Backing {
    TempFile(Mutex<File>),
    Memory {
        limit_blocks: usize,
        blocks: Mutex<BTreeMap<usize, Box<[u8]>>>,
    },
}

impl Store {
    /// The whole file in a temporary file in `dir`, deleted when the store is dropped.
    pub fn temp_file(dir: &Path, file_size: usize) -> io::Result<Self> {
        // unlinked at once: only this handle keeps it
        let file = NamedTempFile::new_in(dir)?.into_file();
        file.set_len(file_size as u64)?;
        Ok(Store {
            file_size,
            backing: Backing::TempFile(Mutex::new(file)),
        })
    }

    /// At most `limit` bytes of the file in memory (at least four blocks).
    pub fn memory(file_size: usize, limit: usize) -> Self {
        Store {
            file_size,
            backing: Backing::Memory {
                limit_blocks: (limit / BLOCK).max(4),
                blocks: Mutex::new(BTreeMap::new()),
            },
        }
    }

    /// Does the whole file fit? Then nothing is ever dropped.
    pub fn holds_all(&self) -> bool {
        match &self.backing {
            Backing::TempFile(_) => true,
            Backing::Memory { limit_blocks, .. } => self.file_size.div_ceil(BLOCK) <= *limit_blocks,
        }
    }

    /// How far ahead of the reader the downloader may fetch: the limit, less a quarter kept for
    /// seeks back.
    pub fn ahead(&self) -> usize {
        match &self.backing {
            Backing::Memory { limit_blocks, .. } if !self.holds_all() => {
                limit_blocks * BLOCK * 3 / 4
            }
            _ => self.file_size,
        }
    }

    fn block_len(&self, b: usize) -> usize {
        BLOCK.min(self.file_size - b * BLOCK)
    }

    /// `data` at `offset`, with the reader at `reader`. Returns the ranges dropped to make room
    /// (never one this write touches).
    pub fn write(&self, offset: usize, data: &[u8], reader: usize) -> io::Result<Vec<Range>> {
        let mut evicted = Vec::new();
        if data.is_empty() || offset >= self.file_size {
            return Ok(evicted);
        }
        let end = (offset + data.len()).min(self.file_size);
        let data = &data[..end - offset];
        let (limit_blocks, blocks) = match &self.backing {
            Backing::TempFile(file) => {
                let mut file = file.lock().expect(STORE_POISON_MSG);
                file.seek(SeekFrom::Start(offset as u64))?;
                file.write_all(data)?;
                return Ok(evicted);
            }
            Backing::Memory {
                limit_blocks,
                blocks,
            } => (*limit_blocks, blocks),
        };
        let mut blocks = blocks.lock().expect(STORE_POISON_MSG);
        let (first, last) = (offset / BLOCK, (end - 1) / BLOCK);
        let reader_block = reader / BLOCK;
        for b in first..=last {
            if !blocks.contains_key(&b) {
                while blocks.len() >= limit_blocks {
                    let behind = blocks
                        .keys()
                        .copied()
                        .find(|&k| k < reader_block && !(first..=last).contains(&k));
                    let ahead = blocks
                        .keys()
                        .rev()
                        .copied()
                        .find(|&k| k > last || k < first);
                    let Some(victim) = behind.or(ahead) else {
                        break;
                    };
                    blocks.remove(&victim);
                    evicted.push(Range::new(victim * BLOCK, self.block_len(victim)));
                }
                blocks.insert(b, vec![0u8; self.block_len(b)].into_boxed_slice());
            }
            let block = blocks.get_mut(&b).expect("block was just inserted");
            let (from, to) = ((b * BLOCK).max(offset), ((b + 1) * BLOCK).min(end));
            block[from - b * BLOCK..to - b * BLOCK]
                .copy_from_slice(&data[from - offset..to - offset]);
        }
        Ok(evicted)
    }

    /// Bytes from `offset` into `out`, as far as they are stored (the caller checked that they
    /// were downloaded).
    pub fn read(&self, offset: usize, out: &mut [u8]) -> io::Result<usize> {
        let len = out.len().min(self.file_size.saturating_sub(offset));
        let out = &mut out[..len];
        let blocks = match &self.backing {
            Backing::TempFile(file) => {
                let mut file = file.lock().expect(STORE_POISON_MSG);
                file.seek(SeekFrom::Start(offset as u64))?;
                return file.read(out);
            }
            Backing::Memory { blocks, .. } => blocks.lock().expect(STORE_POISON_MSG),
        };
        let mut done = 0;
        while done < out.len() {
            let pos = offset + done;
            let Some(block) = blocks.get(&(pos / BLOCK)) else {
                break;
            };
            let at = pos % BLOCK;
            let n = (block.len() - at).min(out.len() - done);
            out[done..done + n].copy_from_slice(&block[at..at + n]);
            done += n;
        }
        Ok(done)
    }
}

/// The whole file from the start, once all of it is stored (for the cache).
pub(super) struct WholeFile<'a> {
    pub store: &'a Store,
    pub pos: usize,
}

impl Read for WholeFile<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let n = self.store.read(self.pos, out)?;
        self.pos += n;
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn whole(store: &Store) -> Vec<u8> {
        let mut out = Vec::new();
        WholeFile { store, pos: 0 }.read_to_end(&mut out).unwrap();
        out
    }

    #[test]
    fn evicts_behind_the_reader_first() {
        let s = Store::memory(BLOCK * 10, BLOCK * 4);
        let data = vec![7u8; BLOCK];
        for b in 0..4 {
            assert!(s.write(b * BLOCK, &data, 0).unwrap().is_empty());
        }
        // reader at block 3: block 0 goes first
        let e = s.write(4 * BLOCK, &data, 3 * BLOCK).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].start, 0);
        let mut out = vec![0u8; 10];
        assert_eq!(s.read(4 * BLOCK, &mut out).unwrap(), 10);
        assert_eq!(s.read(0, &mut out).unwrap(), 0);
    }

    #[test]
    fn evicts_ahead_when_nothing_is_behind() {
        let s = Store::memory(BLOCK * 10, BLOCK * 4);
        let data = vec![1u8; BLOCK];
        for b in [0, 5, 6, 7] {
            s.write(b * BLOCK, &data, 0).unwrap();
        }
        let e = s.write(BLOCK, &data, 0).unwrap();
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].start, 7 * BLOCK);
    }

    #[test]
    fn writes_across_blocks() {
        let s = Store::memory(BLOCK * 3, BLOCK * 8);
        let data: Vec<u8> = (0..BLOCK + 10).map(|i| i as u8).collect();
        s.write(BLOCK - 5, &data, 0).unwrap();
        let mut out = vec![0u8; BLOCK + 10];
        assert_eq!(s.read(BLOCK - 5, &mut out).unwrap(), BLOCK + 10);
        assert_eq!(out, data);
        assert!(s.holds_all());
    }

    #[test]
    fn temp_file_and_memory_hold_the_same_file() {
        let size = BLOCK * 2 + 100;
        let data: Vec<u8> = (0..size).map(|i| (i * 7) as u8).collect();
        let file = Store::temp_file(&std::env::temp_dir(), size).unwrap();
        let memory = Store::memory(size, usize::MAX);
        // out of order
        for (from, to) in [(BLOCK, size), (0, BLOCK)] {
            for s in [&file, &memory] {
                assert!(s.write(from, &data[from..to], 0).unwrap().is_empty());
            }
        }
        assert!(file.holds_all() && memory.holds_all());
        assert_eq!(file.ahead(), size);
        assert_eq!(whole(&file), data);
        assert_eq!(whole(&memory), data);
    }
}
