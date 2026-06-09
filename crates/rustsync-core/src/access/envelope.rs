use aes_gcm::{
    KeyInit,
    aead::{Aead, Payload},
};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use rand::RngCore;
use rand_core::OsRng;
use rustsync_protocol::{
    DeviceId, DeviceRecord, EnvelopeAlgorithm, KeyEnvelope, KeyId, WorkspacePermission,
};
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    access::AccessState,
    device::DeviceIdentity,
    keyring::{KeyVisibility, WORKSPACE_KEY_SIZE, WorkspaceKey},
};

use super::{AccessError, AccessResult};

const ENVELOPE_HKDF_SALT: &[u8] = b"rustsync/key-envelope/hkdf-sha256";

pub fn authorize_key_delivery(
    state: &AccessState,
    visibility: KeyVisibility,
    key_id: &KeyId,
    generation: u64,
    sender_device_id: &DeviceId,
    recipient_device_id: &DeviceId,
) -> AccessResult<()> {
    state.require_permission(sender_device_id, WorkspacePermission::ManageKeys)?;

    state.active_membership(recipient_device_id)?;

    if visibility == KeyVisibility::Restricted {
        state.require_key_grant(key_id, generation, recipient_device_id)?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn seal_key_envelope(
    state: &AccessState,
    visibility: KeyVisibility,
    key_id: KeyId,
    generation: u64,
    workspace_key: &WorkspaceKey,
    sender: &DeviceIdentity,
    sender_record: &DeviceRecord,
    recipient: &DeviceRecord,
    created_at: u64,
) -> AccessResult<KeyEnvelope> {
    sender.validate()?;
    sender_record.validate()?;
    recipient.validate()?;

    require_identity_matches_record(sender, sender_record)?;

    authorize_key_delivery(
        state,
        visibility,
        &key_id,
        generation,
        &sender_record.device_id,
        &recipient.device_id,
    )?;

    let ephemeral_secret = StaticSecret::random_from_rng(OsRng);
    let ephemeral_public = PublicKey::from(&ephemeral_secret);
    let recipient_public = PublicKey::from(recipient.exchange_public_key);
    let shared_secret = ephemeral_secret.diffie_hellman(&recipient_public);

    let mut envelope = KeyEnvelope {
        workspace_id: state.workspace_id().clone(),
        key_id,
        key_generation: generation,
        access_revision: state.revision(),
        sender_device_id: sender_record.device_id.clone(),
        recipient_device_id: recipient.device_id.clone(),
        algorithm: EnvelopeAlgorithm::X25519HkdfSha256XChaCha20Poly1305,
        sender_ephemeral_public_key: ephemeral_public.to_bytes(),
        nonce: [0; 24],
        encrypted_workspace_key: Vec::new(),
        created_at,
        signature: Vec::new(),
    };

    let context = envelope.context_bytes();
    let envelope_key = derive_envelope_key(shared_secret.as_bytes(), &context)?;

    OsRng.fill_bytes(&mut envelope.nonce);

    let cipher = XChaCha20Poly1305::new_from_slice(&envelope_key)
        .map_err(|_| AccessError::EnvelopeEncryptionFailed)?;

    envelope.encrypted_workspace_key = cipher
        .encrypt(
            XNonce::from_slice(&envelope.nonce),
            Payload {
                msg: workspace_key.expose_secret(),
                aad: &context,
            },
        )
        .map_err(|_| AccessError::EnvelopeEncryptionFailed)?;

    envelope.signature = sender.sign(&envelope.signing_payload())?;

    Ok(envelope)
}

pub fn open_key_envelope(
    state: &AccessState,
    visibility: KeyVisibility,
    envelope: &KeyEnvelope,
    recipient: &DeviceIdentity,
    recipient_record: &DeviceRecord,
    sender_record: &DeviceRecord,
) -> AccessResult<WorkspaceKey> {
    if envelope.workspace_id != *state.workspace_id() {
        return Err(AccessError::WorkspaceMismatch {
            expected: state.workspace_id().clone(),
            actual: envelope.workspace_id.clone(),
        });
    }

    if envelope.access_revision > state.revision() {
        return Err(AccessError::AccessStateTooOld {
            local_revision: state.revision(),
            envelope_revision: envelope.access_revision,
        });
    }

    recipient.validate()?;
    recipient_record.validate()?;
    sender_record.validate()?;

    require_identity_matches_record(recipient, recipient_record)?;

    if envelope.recipient_device_id != recipient_record.device_id {
        return Err(AccessError::WrongEnvelopeRecipient {
            expected: recipient_record.device_id.clone(),
            actual: envelope.recipient_device_id.clone(),
        });
    }

    authorize_key_delivery(
        state,
        visibility,
        &envelope.key_id,
        envelope.key_generation,
        &sender_record.device_id,
        &recipient_record.device_id,
    )?;

    envelope.verify_sender_signature(sender_record)?;

    let recipient_secret = StaticSecret::from(*recipient.exchange_private_key());
    let ephemeral_public = PublicKey::from(envelope.sender_ephemeral_public_key);
    let shared_secret = recipient_secret.diffie_hellman(&ephemeral_public);

    let context = envelope.context_bytes();
    let envelope_key = derive_envelope_key(shared_secret.as_bytes(), &context)?;

    let cipher = XChaCha20Poly1305::new_from_slice(&envelope_key)
        .map_err(|_| AccessError::EnvelopeDecryptionFailed)?;

    let decrypted = cipher
        .decrypt(
            XNonce::from_slice(&envelope.nonce),
            Payload {
                msg: &envelope.encrypted_workspace_key,
                aad: &context,
            },
        )
        .map_err(|_| AccessError::EnvelopeDecryptionFailed)?;

    let actual = decrypted.len();

    WorkspaceKey::try_from_vec(decrypted).map_err(|_| AccessError::InvalidWorkspaceKeyLength {
        expected: WORKSPACE_KEY_SIZE,
        actual,
    })
}

fn require_identity_matches_record(
    identity: &DeviceIdentity,
    record: &DeviceRecord,
) -> AccessResult<()> {
    if identity.device_id() != &record.device_id
        || identity.signing_public_key() != &record.signing_public_key
        || identity.exchange_public_key() != &record.exchange_public_key
    {
        return Err(AccessError::DeviceIdentityConflict {
            device_id: record.device_id.clone(),
        });
    }

    Ok(())
}

fn derive_envelope_key(shared_secret: &[u8; 32], context: &[u8]) -> AccessResult<[u8; 32]> {
    let hkdf = Hkdf::<Sha256>::new(Some(ENVELOPE_HKDF_SALT), shared_secret);

    let mut key = [0; 32];

    hkdf.expand(context, &mut key)
        .map_err(|_| AccessError::KeyDerivationFailed)?;

    Ok(key)
}
