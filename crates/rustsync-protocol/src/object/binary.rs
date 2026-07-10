use super::{ContentEncryptionAlgorithm, EncryptedObject, XCHACHA20_POLY1305_NONCE_SIZE};
use crate::{KeyId, ProtocolError, ProtocolResult};

pub(super) const MAGIC: &[u8; 4] = b"RSOB";
const VERSION: u8 = 1;
const XCHACHA20_POLY1305_TAG: u8 = 1;
const FIXED_HEADER_LEN: usize = MAGIC.len() + 1 + 1 + 2 + 1;

pub(super) fn encode(object: &EncryptedObject) -> ProtocolResult<Vec<u8>> {
    object.validate()?;

    let key_id = object.key_id.as_str().as_bytes();
    let key_id_len =
        u16::try_from(key_id.len()).map_err(|_| ProtocolError::InvalidEncryptedObjectBinary)?;
    let capacity = FIXED_HEADER_LEN
        .checked_add(key_id.len())
        .and_then(|length| length.checked_add(object.nonce.len()))
        .and_then(|length| length.checked_add(object.ciphertext.len()))
        .ok_or(ProtocolError::InvalidEncryptedObjectBinary)?;

    let mut bytes = Vec::with_capacity(capacity);
    bytes.extend_from_slice(MAGIC);
    bytes.push(VERSION);
    bytes.push(XCHACHA20_POLY1305_TAG);
    bytes.extend_from_slice(&key_id_len.to_be_bytes());
    bytes.push(u8::try_from(object.nonce.len()).map_err(|_| {
        ProtocolError::InvalidNonceLength {
            expected: XCHACHA20_POLY1305_NONCE_SIZE,
            actual: object.nonce.len(),
        }
    })?);
    bytes.extend_from_slice(key_id);
    bytes.extend_from_slice(&object.nonce);
    bytes.extend_from_slice(&object.ciphertext);
    Ok(bytes)
}

pub(super) fn decode(bytes: &[u8]) -> ProtocolResult<EncryptedObject> {
    let mut cursor = Cursor::new(bytes);
    if cursor.take(MAGIC.len()) != Some(MAGIC.as_slice()) {
        return Err(ProtocolError::InvalidEncryptedObjectBinary);
    }

    let version = cursor.byte()?;
    if version != VERSION {
        return Err(ProtocolError::UnsupportedEncryptedObjectVersion(version));
    }

    let algorithm = match cursor.byte()? {
        XCHACHA20_POLY1305_TAG => ContentEncryptionAlgorithm::XChaCha20Poly1305,
        tag => return Err(ProtocolError::UnsupportedContentEncryptionAlgorithm(tag)),
    };
    let key_id_len = usize::from(u16::from_be_bytes(cursor.array()?));
    let nonce_len = usize::from(cursor.byte()?);
    let key_id_bytes = cursor
        .take(key_id_len)
        .ok_or(ProtocolError::InvalidEncryptedObjectBinary)?;
    let key_id_text = std::str::from_utf8(key_id_bytes)
        .map_err(|_| ProtocolError::InvalidEncryptedObjectBinary)?;
    let key_id =
        KeyId::parse(key_id_text).map_err(|_| ProtocolError::InvalidEncryptedObjectBinary)?;
    let nonce = cursor
        .take(nonce_len)
        .ok_or(ProtocolError::InvalidEncryptedObjectBinary)?
        .to_vec();
    let ciphertext = cursor.remaining().to_vec();

    let object = EncryptedObject::new(key_id, algorithm, nonce, ciphertext);
    object.validate()?;
    Ok(object)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Cursor<'a> {
    const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn take(&mut self, length: usize) -> Option<&'a [u8]> {
        let end = self.position.checked_add(length)?;
        let value = self.bytes.get(self.position..end)?;
        self.position = end;
        Some(value)
    }

    fn byte(&mut self) -> ProtocolResult<u8> {
        self.take(1)
            .and_then(|bytes| bytes.first().copied())
            .ok_or(ProtocolError::InvalidEncryptedObjectBinary)
    }

    fn array(&mut self) -> ProtocolResult<[u8; 2]> {
        self.take(2)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(ProtocolError::InvalidEncryptedObjectBinary)
    }

    fn remaining(&self) -> &'a [u8] {
        self.bytes.get(self.position..).unwrap_or_default()
    }
}
