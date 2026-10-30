//! Model files (L3).
//!
//! For now a model is JSON with a format header. Step 2 replaces the body with a binary,
//! chunked layout that streams hundreds of thousands of elements without building a JSON tree.

use serde::{Deserialize, Serialize};
use structurecraft_model::Model;

/// The file extension of a StructureCraft model.
pub const EXTENSION: &str = "stc";

/// The newest format version this build writes. Older versions still read.
pub const FORMAT_VERSION: u32 = 1;

const MAGIC: &str = "structurecraft";

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not a StructureCraft model: {0}")]
    Malformed(#[from] serde_json::Error),
    #[error("not a StructureCraft model")]
    WrongMagic,
    #[error("made by a newer StructureCraft (format {0}, this build reads up to {FORMAT_VERSION})")]
    TooNew(u32),
}

#[derive(Serialize, Deserialize)]
struct File<M> {
    format: String,
    version: u32,
    model: M,
}

pub fn to_bytes(model: &Model) -> Result<Vec<u8>, Error> {
    Ok(serde_json::to_vec_pretty(&File { format: MAGIC.to_owned(), version: FORMAT_VERSION, model })?)
}

pub fn from_bytes(bytes: &[u8]) -> Result<Model, Error> {
    let file: File<serde_json::Value> = serde_json::from_slice(bytes)?;
    if file.format != MAGIC {
        return Err(Error::WrongMagic);
    }
    if file.version > FORMAT_VERSION {
        return Err(Error::TooNew(file.version));
    }
    Ok(serde_json::from_value(file.model)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let model = Model::new("Tower A");
        assert_eq!(from_bytes(&to_bytes(&model).unwrap()).unwrap(), model);
    }

    #[test]
    fn hostile_bytes_are_errors_not_panics() {
        for bytes in [&b""[..], b"\xff\xfe", b"{}", b"[1,2]", br#"{"format":"other","version":1,"model":{}}"#] {
            assert!(from_bytes(bytes).is_err());
        }
        let newer = br#"{"format":"structurecraft","version":999,"model":{}}"#;
        assert!(matches!(from_bytes(newer), Err(Error::TooNew(999))));
    }
}
