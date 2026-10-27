use super::thread_history::{completed_item, event_body, is_completed_item, RolloutEnvelope};
use anyhow::{anyhow, bail, Context, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::Duration;

pub(super) const ITEM_BODY_COLUMNS: &str =
    "source_rollout_id,record_byte_offset,record_byte_length,item_bytes,item_digest";

#[derive(Debug, Clone)]
pub(super) struct ItemBodyRef {
    pub rollout_id: String,
    pub offset: u64,
    pub length: u64,
    pub item_bytes: usize,
    pub digest: Vec<u8>,
}

impl ItemBodyRef {
    pub(super) fn from_row(row: &rusqlite::Row<'_>, start: usize) -> rusqlite::Result<Self> {
        Ok(Self {
            rollout_id: row.get(start)?,
            offset: row.get(start + 1)?,
            length: row.get(start + 2)?,
            item_bytes: row.get(start + 3)?,
            digest: row.get(start + 4)?,
        })
    }
}

pub(super) fn item_digest(item: &Value) -> Result<(usize, Vec<u8>)> {
    let encoded = serde_json::to_vec(item)?;
    Ok((encoded.len(), Sha256::digest(&encoded).to_vec()))
}

pub(super) fn open_catalog(root: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        root.join("state_1.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.busy_timeout(Duration::from_secs(2))?;
    Ok(conn)
}

pub(super) fn source_path(conn: &Connection, root: &Path, rollout: &str) -> Result<PathBuf> {
    let relative: String = conn
        .query_row(
            "SELECT rollout_path FROM canonical_rollouts WHERE rollout_id=?1",
            [rollout],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| anyhow!("canonical history source is missing"))?;
    super::thread_store::checked_rollout_path(root, &root.join(relative))
}

pub(super) struct ItemBodyReader {
    root: PathBuf,
    catalog: Option<Connection>,
    readers: HashMap<String, super::rollout_compression::RolloutReader>,
}

impl ItemBodyReader {
    pub(super) fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            catalog: None,
            readers: HashMap::new(),
        }
    }

    pub(super) fn for_connection(conn: &Connection) -> Result<Self> {
        let path = conn
            .path()
            .ok_or_else(|| anyhow!("history database has no filesystem path"))?;
        Ok(Self::new(Path::new(path).parent().ok_or_else(|| {
            anyhow!("history database has no parent")
        })?))
    }

    pub(super) fn read(
        &mut self,
        reference: &ItemBodyRef,
        ordinal: u64,
        id: &str,
    ) -> Result<Value> {
        if reference.length == 0
            || reference.length > super::rollout::ROLLOUT_MAX_RECORD_BYTES as u64 + 2
            || reference.item_bytes > super::rollout::ROLLOUT_MAX_RECORD_BYTES
            || reference.digest.len() != 32
        {
            bail!("invalid canonical history body locator");
        }
        if !self.readers.contains_key(&reference.rollout_id) {
            if self.catalog.is_none() {
                self.catalog = Some(open_catalog(&self.root)?);
            }
            let catalog = self.catalog.as_ref().unwrap();
            let path = source_path(catalog, &self.root, &reference.rollout_id)?;
            let reader = match super::rollout_compression::open_rollout_reader(&path) {
                Ok(reader) => reader,
                Err(_) => {
                    let path = source_path(catalog, &self.root, &reference.rollout_id)?;
                    super::rollout_compression::open_rollout_reader(path)?
                }
            };
            if self.readers.len() >= 64 {
                bail!("history body read exceeds lineage limit");
            }
            self.readers.insert(reference.rollout_id.clone(), reader);
        }
        let reader = self.readers.get_mut(&reference.rollout_id).unwrap();
        let end = reference
            .offset
            .checked_add(reference.length)
            .ok_or_else(|| anyhow!("canonical history body offset overflow"))?;
        if end > reader.len() {
            bail!("canonical history body is outside the durable source");
        }
        reader.seek(SeekFrom::Start(reference.offset))?;
        let mut bytes = vec![0; usize::try_from(reference.length)?];
        reader.read_exact(&mut bytes)?;
        if bytes.last() != Some(&b'\n') {
            bail!("canonical history body is not a complete record");
        }
        let envelope: RolloutEnvelope =
            serde_json::from_slice(&bytes).context("invalid canonical history body record")?;
        if !is_completed_item(&envelope) {
            bail!("canonical history body is not a completed item");
        }
        let item = completed_item(event_body(&envelope))?;
        let actual_id = item
            .get("id")
            .and_then(Value::as_str)
            .or_else(|| item.get("item_id").and_then(Value::as_str))
            .or_else(|| event_body(&envelope).get("item_id").and_then(Value::as_str));
        if envelope.ordinal != ordinal || actual_id != Some(id) {
            bail!("canonical history body identity does not match its index");
        }
        let (size, digest) = item_digest(item)?;
        if size != reference.item_bytes || digest != reference.digest {
            bail!("canonical history body does not match its immutable index");
        }
        Ok(item.clone())
    }
}
