use rustsync_protocol::{ContentEncryptionAlgorithm, EncryptedObject, KeyId, ProtocolError};

fn encrypted_object() -> EncryptedObject {
    EncryptedObject::new(
        KeyId::parse("main").expect("valid key id"),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        (0..24).collect(),
        vec![
            0xde, 0xad, 0xbe, 0xef, 0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xa0,
            0xb0, 0xc0,
        ],
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
            0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xa0, 0xb0, 0xc0,
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
fn rsob_v1_golden_fixture_is_stable_in_both_directions() {
    let fixture = include_bytes!("fixtures/encrypted-object-v1.rsob");
    let expected = EncryptedObject::new(
        KeyId::parse("main").expect("valid key id"),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        (0..24).collect(),
        (0x10..=0x1f).collect(),
    );

    assert_eq!(
        EncryptedObject::from_binary_bytes(fixture).expect("decode frozen fixture"),
        expected
    );
    assert_eq!(
        expected.to_binary_bytes().expect("encode expected object"),
        fixture
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

    // Every prefix through the fixed header, key ID, and nonce is incomplete
    // and must be rejected; trailing bytes are ciphertext by the v1 grammar.
    for truncated in 0..37 {
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

    let empty_ciphertext = &bytes[..bytes.len() - 16];
    assert!(matches!(
        EncryptedObject::from_binary_bytes(empty_ciphertext),
        Err(ProtocolError::EmptyCiphertext)
    ));
}

#[test]
fn binary_decoder_rejects_ciphertext_shorter_than_an_authentication_tag() {
    let mut bytes = encrypted_object().to_binary_bytes().expect("encode object");
    bytes.pop();

    assert!(matches!(
        EncryptedObject::from_binary_bytes(&bytes),
        Err(ProtocolError::CiphertextTooShort {
            minimum: 16,
            actual: 15
        })
    ));
}

#[test]
fn binary_decoder_never_panics_on_malformed_vectors() {
    let fixture = include_bytes!("fixtures/encrypted-object-v1.rsob");
    let mut unsupported_algorithm = fixture.to_vec();
    unsupported_algorithm[5] = u8::MAX;
    let mut oversized_key_id = fixture.to_vec();
    oversized_key_id[6..8].copy_from_slice(&u16::MAX.to_be_bytes());

    let mut vectors = vec![
        Vec::new(),
        vec![0xff; 64],
        br#"{\"key_id\":\"main\"}"#.to_vec(),
        unsupported_algorithm,
        oversized_key_id,
    ];
    vectors.extend((0..fixture.len()).map(|end| fixture[..end].to_vec()));

    for vector in vectors {
        assert!(
            std::panic::catch_unwind(|| { EncryptedObject::from_binary_bytes(&vector) }).is_ok()
        );
    }
}

#[test]
fn binary_codec_rejects_oversized_frames() {
    let oversized = EncryptedObject::new(
        KeyId::parse("main").expect("valid key id"),
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        vec![0; 24],
        vec![0; 1_048_576],
    );

    assert!(oversized.to_binary_bytes().is_err());
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
