use rustsync_protocol::{ContentEncryptionAlgorithm, EncryptedObject, KeyId, ProtocolError};

#[test]
fn encrypted_object_rejects_empty_ciphertext() {
    let key_id = KeyId::parse("main").expect("valid key id");
    let object = EncryptedObject::new(
        key_id,
        ContentEncryptionAlgorithm::XChaCha20Poly1305,
        vec![1; 24],
        Vec::new(),
    );

    let error = object
        .validate()
        .expect_err("empty ciphertext should fail validation");

    assert!(matches!(error, ProtocolError::EmptyCiphertext));
}
