//! Inert metadata from an audited Mach-O Bun standalone graph. No bootstrap,
//! bytecode, native module, resource extraction or lifecycle code is executed.
use super::*;
use std::cell::RefCell;
use std::io::{Seek, SeekFrom};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Contract {
    pub binary_bytes: u64,
    pub table_sha256: String,
    pub entry_name: String,
    pub guards: Vec<electron_package::Guard>,
    pub resources: BTreeMap<String, String>,
}
struct Member { offset: u64, bytes: u64, encoding: u8 }
pub(super) struct Graph {
    path: PathBuf,
    file: RefCell<fs::File>,
    before: fs::Metadata,
    members: BTreeMap<String, Member>,
    resources: BTreeMap<String, String>,
    json_guards: BTreeMap<String, String>,
}
fn word(data: &[u8], at: usize) -> Result<u32, ()> {
    Ok(u32::from_le_bytes(data.get(at..at+4).ok_or(())?.try_into().map_err(|_| ())?))
}
fn wide(data: &[u8], at: usize) -> Result<u64, ()> {
    Ok(u64::from_le_bytes(data.get(at..at+8).ok_or(())?.try_into().map_err(|_| ())?))
}
fn read(file: &mut fs::File, offset: u64, count: u64, length: u64, budget: &mut ScanBudget) -> Result<Vec<u8>, ()> {
    budget.visit()?;
    if offset.checked_add(count).is_none_or(|end| end > length)
        || count > MAX_VERIFIED_BYTES.saturating_sub(budget.verified_bytes) as u64 { return Err(()); }
    let mut data = vec![0; usize::try_from(count).map_err(|_| ())?];
    file.seek(SeekFrom::Start(offset)).map_err(|_| ())?;
    file.read_exact(&mut data).map_err(|_| ())?;
    budget.verified_bytes += data.len(); Ok(data)
}
impl Graph {
    pub(super) fn open(path: &Path, contract: &Contract, budget: &mut ScanBudget) -> Result<Self, ()> {
        let mut file = open_confined_file(path)?; let before = file.metadata().map_err(|_| ())?;
        let length = before.len();
        if !before.is_file() || length != contract.binary_bytes { return Err(()); }
        let header = read(&mut file, 0, 32, length, budget)?;
        if word(&header, 0)? != 0xfeedfacf || word(&header, 4)? != 0x0100000c { return Err(()); }
        let count = word(&header, 16)? as usize; let size = word(&header, 20)? as usize;
        if count > 128 || size > 1_048_576 { return Err(()); }
        let commands = read(&mut file, 32, size as u64, length, budget)?;
        let mut at = 0; let mut section = None;
        for _ in 0..count {
            let kind = word(&commands, at)?; let size = word(&commands, at+4)? as usize;
            if size < 8 || at.checked_add(size).is_none_or(|end| end > commands.len()) { return Err(()); }
            if kind == 0x19 {
                let sections = word(&commands, at+64)? as usize;
                if sections > 128 || 72 + sections*80 > size { return Err(()); }
                for index in 0..sections {
                    let row = at+72+index*80;
                    if commands.get(row..row+16).ok_or(())?.strip_suffix(&[0; 11]) == Some(b"__bun".as_slice())
                        && commands.get(row+16..row+32).ok_or(())?.strip_suffix(&[0; 11]) == Some(b"__BUN".as_slice()) {
                        if section.is_some() { return Err(()); }
                        section = Some((word(&commands, row+48)? as u64, wide(&commands, row+40)?));
                    }
                }
            }
            at += size;
        }
        if at != commands.len() { return Err(()); }
        let (offset, size) = section.ok_or(())?;
        if offset.checked_add(size).is_none_or(|end| end > length) { return Err(()); }
        let graph_bytes = wide(&read(&mut file, offset, 8, length, budget)?, 0)?;
        if graph_bytes < 48 || graph_bytes > size.checked_sub(8).ok_or(())? { return Err(()); }
        let base = offset.checked_add(8).ok_or(())?;
        let footer = read(&mut file, base+graph_bytes-48, 48, length, budget)?;
        if footer.get(32..) != Some(b"\n---- Bun! ----\n".as_slice()) { return Err(()); }
        let payload = wide(&footer, 0)?;
        if payload != graph_bytes-48 { return Err(()); }
        let table_offset = word(&footer, 8)? as u64; let table_bytes = word(&footer, 12)? as u64;
        let entry = word(&footer, 16)? as usize;
        if table_bytes == 0 || table_bytes%52 != 0 || table_bytes/52 > MAX_ENTRIES as u64
            || table_offset.checked_add(table_bytes).is_none_or(|end| end > payload)
            || (word(&footer,20)? as u64).checked_add(word(&footer,24)? as u64).is_none_or(|end| end > payload) { return Err(()); }
        let table = read(&mut file, base+table_offset, table_bytes, length, budget)?;
        if hex::encode(Sha256::digest(&table)) != contract.table_sha256 { return Err(()); }
        let mut members = BTreeMap::new(); let mut entry_name = None;
        for (index, row) in table.chunks_exact(52).enumerate() {
            for at in (0..48).step_by(8) {
                if (word(row,at)? as u64).checked_add(word(row,at+4)? as u64).is_none_or(|end| end > payload) { return Err(()); }
            }
            let name_bytes = word(row,4)? as u64;
            if name_bytes == 0 || name_bytes > 4096 || (word(row,0)? as u64)+name_bytes >= payload
                || row[48] > 2 || row[50] > 2 || row[51] > 1 { return Err(()); }
            let raw = read(&mut file, base+word(row,0)? as u64, name_bytes+1, length, budget)?;
            if raw.last() != Some(&0) { return Err(()); }
            let name = std::str::from_utf8(&raw[..raw.len()-1]).map_err(|_| ())?.to_owned();
            let tail = name.strip_prefix("/$bunfs/root/").ok_or(())?;
            if qoder::local_path(tail)? != tail { return Err(()); }
            if index == entry { entry_name = Some(name.clone()); }
            let bytes = word(row,12)? as u64;
            if row[48] == 2 && bytes%2 != 0 { return Err(()); }
            if members.insert(name, Member { offset:base+word(row,8)? as u64, bytes, encoding:row[48] }).is_some() { return Err(()); }
        }
        if entry_name.as_deref() != Some(&contract.entry_name) || contract.guards.is_empty() || contract.guards.len() > 64 { return Err(()); }
        let mut json_guards = BTreeMap::new();
        for guard in &contract.guards {
            let member = members.get(&guard.path).ok_or(())?;
            if member.bytes != guard.bytes { return Err(()); }
            let data = read(&mut file, member.offset, member.bytes, length, budget)?;
            if hex::encode(Sha256::digest(data)) != guard.sha256 { return Err(()); }
            if guard.path.ends_with(".json") { json_guards.insert(guard.path.clone(),guard.sha256.clone()); }
        }
        for (logical, name) in &contract.resources {
            if qoder::local_path(logical)? != *logical || !members.contains_key(name) { return Err(()); }
        }
        if contract.resources.len() > MAX_ENTRIES { return Err(()); }
        let graph = Self { path:path.to_path_buf(),file:RefCell::new(file),before,members,resources:contract.resources.clone(),json_guards };
        graph.unchanged()?; Ok(graph)
    }
    pub(super) fn unchanged(&self) -> Result<(), ()> {
        let after = self.file.borrow().metadata().map_err(|_| ())?;
        let bound = fs::symlink_metadata(&self.path).map_err(|_| ())?;
        if !bound.file_type().is_file() || self.before.len() != after.len()
            || self.before.modified().map_err(|_| ())? != after.modified().map_err(|_| ())? { return Err(()); }
        #[cfg(unix)] {
            use std::os::unix::fs::MetadataExt;
            if self.before.dev() != bound.dev() || self.before.ino() != bound.ino()
                || self.before.ctime() != after.ctime() || self.before.ctime_nsec() != after.ctime_nsec() { return Err(()); }
        }
        Ok(())
    }
    pub(super) fn source(&self, name: &str) -> Result<String, ()> {
        if !self.members.contains_key(name) { return Err(()); }
        Ok(format!("{}!{name}",collapse_home(&self.path)))
    }
    pub(super) fn path(&self, logical: &str) -> Result<String, ()> {
        self.source(self.resources.get(logical).ok_or(())?)
    }
    pub(super) fn has(&self, logical: &str) -> bool { self.resources.contains_key(logical) }
    pub(super) fn names(&self) -> impl Iterator<Item=&String> { self.resources.keys() }
    pub(super) fn json(&self, logical: &str, budget: &mut ScanBudget) -> Result<Option<Arc<Value>>, ()> {
        let Some(name) = self.resources.get(logical) else { return Ok(None); };
        let member = self.members.get(name).ok_or(())?;
        let expected = self.json_guards.get(name).ok_or(())?;
        budget.references += 1;
        if budget.references > MAX_REFERENCES || member.bytes > 2_097_152 || member.bytes > MAX_READ_BYTES.saturating_sub(budget.bytes) as u64 { return Err(()); }
        let key = (PathBuf::from(self.source(name)?),DocumentFormat::Json);
        if let Some(value) = budget.json.get(&key) { self.unchanged()?; return Ok(Some(value.clone())); }
        self.unchanged()?;
        let data = read(&mut self.file.borrow_mut(),member.offset,member.bytes,self.before.len(),budget)?;
        self.unchanged()?; budget.bytes += data.len();
        if hex::encode(Sha256::digest(&data)) != *expected { return Err(()); }
        let text = match member.encoding {
            0 => String::from_utf8(data).map_err(|_| ())?,
            1 => data.into_iter().map(char::from).collect(),
            2 => String::from_utf16(&data.chunks_exact(2).map(|b|u16::from_le_bytes([b[0],b[1]])).collect::<Vec<_>>()).map_err(|_| ())?,
            _ => return Err(()),
        };
        let value = Arc::new(serde_json::from_str::<Value>(&text).map_err(|_| ())?);
        budget.json.insert(key,value.clone()); Ok(Some(value))
    }
}
