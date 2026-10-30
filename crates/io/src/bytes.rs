//! Little-endian records with checked reads.

use structurecraft_model::Id;

use crate::Error;

pub struct Writer(Vec<u8>);

impl Writer {
    pub fn with_capacity(n: usize) -> Self {
        Self(Vec::with_capacity(n))
    }

    pub fn finish(self) -> Vec<u8> {
        self.0
    }

    pub fn bytes(&mut self, b: &[u8]) {
        self.0.extend_from_slice(b);
    }

    pub fn json(&mut self, v: &impl serde::Serialize) -> Result<(), Error> {
        serde_json::to_writer(&mut self.0, v)?;
        Ok(())
    }

    pub fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    pub fn u16(&mut self, v: u16) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn u32(&mut self, v: u32) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.bytes(&v.to_le_bytes());
    }

    pub fn id<T>(&mut self, id: Id<T>) {
        self.u32(id.index());
        self.u32(id.generation());
    }

    pub fn len(&mut self, n: usize) -> Result<(), Error> {
        self.u32(u32::try_from(n).map_err(|_| Error::Damaged("too many elements".to_owned()))?);
        Ok(())
    }

    /// A chunk: its tag, its length (filled in after `body` writes it), its body.
    pub fn chunk<E>(&mut self, tag: &[u8; 4], body: impl FnOnce(&mut Self) -> Result<(), E>) -> Result<(), E> {
        self.bytes(tag);
        let at = self.0.len();
        self.bytes(&0u64.to_le_bytes());
        body(self)?;
        let len = (self.0.len() - at - 8) as u64;
        if let Some(slot) = self.0.get_mut(at..at + 8) {
            slot.copy_from_slice(&len.to_le_bytes());
        }
        Ok(())
    }
}

pub struct Reader<'a>(&'a [u8]);

fn damaged(what: &str) -> Error {
    Error::Damaged(format!("{what} runs past the end"))
}

impl<'a> Reader<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn rest(&mut self) -> &'a [u8] {
        std::mem::take(&mut self.0)
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        if n > self.0.len() {
            return Err(damaged("a record"));
        }
        let (head, tail) = self.0.split_at(n);
        self.0 = tail;
        Ok(head)
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| damaged("a value"))
    }

    pub fn tag(&mut self) -> Result<[u8; 4], Error> {
        self.array()
    }

    pub fn u8(&mut self) -> Result<u8, Error> {
        Ok(u8::from_le_bytes(self.array()?))
    }

    pub fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }

    pub fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    pub fn f64(&mut self) -> Result<f64, Error> {
        Ok(f64::from_le_bytes(self.array()?))
    }

    pub fn id<T>(&mut self) -> Result<Id<T>, Error> {
        Ok(Id::new(self.u32()?, self.u32()?))
    }

    /// A chunk body: a u64 length, then that many bytes.
    pub fn take_len(&mut self) -> Result<&'a [u8], Error> {
        let n = u64::from_le_bytes(self.array()?);
        self.take(usize::try_from(n).map_err(|_| damaged("a chunk"))?)
    }

    /// A record count, checked against the bytes left so a hostile count cannot make us
    /// allocate more than the file could hold.
    pub fn count(&mut self, min_record: usize) -> Result<usize, Error> {
        let n = self.u32()? as usize;
        if n.saturating_mul(min_record) > self.0.len() {
            return Err(damaged("a table"));
        }
        Ok(n)
    }
}
