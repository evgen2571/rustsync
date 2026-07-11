use rustsync_protocol::{ContentEncryptionAlgorithm, EncryptedObject, KeyId, ProtocolError};

fn encrypted_object() -> EncryptedObject {
    EncryptedObject::new(
        KeyId::parse("main").expect("valid key id"),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        (0..24).collect(),
        vec![0xde, 0xad, 0xbe, 0xef],
    )
}

#[test]
fn binary_bytes_are_canonical_and_round_trip() {
    let object = encrypted_object();

    let bytes = object.to_binary_bytes().expect("encode object");

    assert_eq!(
        bytes,
        [
            b'R', b'S', b'O', b'B', 1, 1, 0, 4, 24, b'm', b'a', b'i', b'n', 0, 1, 2, 3, 4, 5, 6, 7,
            8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 0xde, 0xad, 0xbe, 0xef,
        ]
    );
    assert!(bytes.starts_with(b"RSOB"));
    assert!(
        !bytes
            .windows(b"key_id".len())
            .any(|window| window == b"key_id")
    );
    assert_eq!(
        object.to_binary_bytes().expect("encode deterministically"),
        bytes
    );
    assert_eq!(
        EncryptedObject::from_binary_bytes(&bytes).expect("decode object"),
        object
    );
}

#[test]
fn binary_encoder_rejects_invalid_objects() {
    let invalid = EncryptedObject::new(
        KeyId::parse("main").expect("valid key id"),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        vec![0; 23],
        Vec::new(),
    );

    assert!(matches!(
        invalid.to_binary_bytes(),
        Err(ProtocolError::EmptyCiphertext)
    ));
}

#[test]
fn binary_decoder_rejects_invalid_frames_without_fallback() {
    let bytes = encrypted_object().to_binary_bytes().expect("encode object");

    let mut wrong_magic = bytes.clone();
    wrong_magic[..4].copy_from_slice(b"NOPE");
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&wrong_magic),
        Err(ProtocolError::InvalidEncryptedObjectBinary)
    ));
    assert!(matches!(
        EncryptedObject::from_remote_bytes(&wrong_magic),
        Err(ProtocolError::InvalidEncryptedObjectEncoding)
    ));

    assert!(matches!(
        EncryptedObject::from_remote_bytes(br#"{"key_id":"main","ciphertext":"not-rsob"}"#),
        Err(ProtocolError::InvalidEncryptedObjectEncoding)
    ));

    let mut unsupported_version = bytes.clone();
    unsupported_version[4] = 2;
    assert!(matches!(
        EncryptedObject::from_remote_bytes(&unsupported_version),
        Err(ProtocolError::UnsupportedEncryptedObjectVersion(2))
    ));

    let mut unknown_algorithm = bytes.clone();
    unknown_algorithm[5] = 99;
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&unknown_algorithm),
        Err(ProtocolError::UnsupportedContentEncryptionAlgorithm(99))
    ));
}

#[test]
fn binary_decoder_rejects_malformed_fields() {
    let bytes = encrypted_object().to_binary_bytes().expect("encode object");

    // Every cursor boundary through the nonce must be rejected:
    // no field may be read from a truncated frame.
    for truncated in [0, 3, 4, 5, 6, 7, 8, 9, 10, 12, 13, 36] {
        assert!(matches!(
            EncryptedObject::from_binary_bytes(&bytes[..truncated]),
            Err(ProtocolError::InvalidEncryptedObjectBinary)
        ));
    }

    let mut impossible_key_id_length = bytes.clone();
    impossible_key_id_length[6..8].copy_from_slice(&u16::MAX.to_be_bytes());
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&impossible_key_id_length),
        Err(ProtocolError::InvalidEncryptedObjectBinary)
    ));

    let mut invalid_utf8 = bytes.clone();
    invalid_utf8[9] = 0xff;
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&invalid_utf8),
        Err(ProtocolError::InvalidEncryptedObjectBinary)
    ));

    let mut invalid_key = bytes.clone();
    invalid_key[9] = b'!';
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&invalid_key),
        Err(ProtocolError::InvalidEncryptedObjectBinary)
    ));

    let mut short_nonce = bytes.clone();
    short_nonce[8] = 23;
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&short_nonce),
        Err(ProtocolError::InvalidNonceLength {
            expected: 24,
            actual: 23
        })
    ));

    let mut long_nonce = bytes.clone();
    long_nonce[8] = 25;
    assert!(matches!(
        EncryptedObject::from_binary_bytes(&long_nonce),
        Err(ProtocolError::InvalidNonceLength {
            expected: 24,
            actual: 25
        })
    ));

    let empty_ciphertext = &bytes[..bytes.len() - 4];
    assert!(matches!(
        EncryptedObject::from_binary_bytes(empty_ciphertext),
        Err(ProtocolError::EmptyCiphertext)
    ));
}

#[test]
fn encrypted_object_rejects_empty_ciphertext() {
    let key_id = KeyId::parse("main").expect("valid key id");
    let object = EncryptedObject::new(
        key_id,
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        vec![1; 24],
        Vec::new(),
    );

    assert!(matches!(
        object.validate(),
        Err(ProtocolError::EmptyCiphertext)
    ));
}
