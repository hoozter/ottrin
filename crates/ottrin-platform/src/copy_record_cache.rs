//! Pure decoding memoization. Callers still read and validate the current file.
use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;

struct Parsed {
    bytes: Arc<[u8]>,
    record: Arc<CopyRecord>,
}

pub(super) struct CopyRecordCache {
    entries: BTreeMap<String, Parsed>,
    bytes: usize,
    limit: usize,
}

impl Default for CopyRecordCache {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            bytes: 0,
            limit: 16 * 1024 * 1024,
        }
    }
}

impl std::fmt::Debug for CopyRecordCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CopyRecordCache")
            .field("entries", &self.entries.len())
            .field("encoded_bytes", &self.bytes)
            .finish()
    }
}

impl CopyRecordCache {
    pub(super) fn uncached() -> Self {
        Self {
            limit: 0,
            ..Self::default()
        }
    }

    pub(super) fn parse(
        &mut self,
        name: &str,
        bytes: Arc<[u8]>,
    ) -> Result<Arc<CopyRecord>, PlatformError> {
        if let Some(parsed) = self.entries.get(name)
            && parsed.bytes == bytes
        {
            return Ok(Arc::clone(&parsed.record));
        }
        // This is the sole decoder and schema validator, including on updates.
        let record = Arc::new(CopyRecord::parse(&bytes)?);
        if let Some(previous) = self.entries.remove(name) {
            self.bytes -= previous.bytes.len();
        }
        if self.bytes.saturating_add(bytes.len()) <= self.limit && self.entries.len() < 4096 {
            self.bytes += bytes.len();
            self.entries.insert(
                name.into(),
                Parsed {
                    bytes,
                    record: Arc::clone(&record),
                },
            );
        }
        Ok(record)
    }
}
