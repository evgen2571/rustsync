use crate::error::EncryptionError;
use base64::{Engine as _, engine::general_purpose};
use rand::RngCore;

#[derive(Debug)]
pub struct EncryptedFile {
    pub key_id: String,
    pub nonce: String,
    pub encrypted_data: Vec<u8>,
}

pub fn encrypt(plain_data: Vec<u8>, key_id: &str) -> Result<EncryptedFile, EncryptionError> {
    let nonce = &generate_nonce();
    let key: &[u8; 32] = &[
        142, 23, 199, 84, 11, 201, 45, 178, 93, 255, 12, 67, 184, 39, 90, 212, 5, 131, 74, 162, 89,
        41, 117, 3, 168, 54, 190, 22, 135, 77, 241, 106,
    ];

    let encrypted_data = chacha20_process(&plain_data, key, nonce)?;

    Ok(EncryptedFile {
        key_id: key_id.to_string(),
        nonce: encode(nonce),
        encrypted_data,
    })
}

pub fn decrypt(
    encrypted_file: &EncryptedFile,
    key_id: &str,
    nonce_str: &str,
) -> Result<Vec<u8>, EncryptionError> {
    let nonce = decode_nonce(nonce_str)?;
    let key: &[u8; 32] = &[
        142, 23, 199, 84, 11, 201, 45, 178, 93, 255, 12, 67, 184, 39, 90, 212, 5, 131, 74, 162, 89,
        41, 117, 3, 168, 54, 190, 22, 135, 77, 241, 106,
    ];
    chacha20_process(&encrypted_file.encrypted_data, key, &nonce)
}

pub fn generate_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

pub fn encode(nonce: &[u8]) -> String {
    general_purpose::STANDARD.encode(nonce)
}

pub fn decode_nonce(encoded: &str) -> Result<[u8; 12], EncryptionError> {
    let bytes = general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| EncryptionError::DecodeError)?;

    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(&bytes);

    Ok(nonce)
}

fn chacha20_process(
    data: &[u8],
    key: &[u8; 32],
    nonce: &[u8; 12],
) -> Result<Vec<u8>, EncryptionError> {
    let mut counter: u32 = 0;
    let mut encrypted_data = Vec::with_capacity(data.len());
    for block in data.chunks(16) {
        let mut result_block = [0u8; 64];
        let mut state = create_initial_state(&key, &nonce, counter);
        let initial_state = create_initial_state(&key, &nonce, counter);
        for _ in 0..10 {
            chacha20_block(&mut state);
        }
        for i in 0..16 {
            state[i] = state[i].wrapping_add(initial_state[i]);
        }
        let keystream = u32_to_bytes_le(&state);
        if size_of_val(block) < 16 {
            let mut temp_vec = block.to_vec();
            temp_vec.resize(16, 0);
            let final_slice: &[u8] = &temp_vec;
            for i in 0..block.len() {
                result_block[i] = final_slice[i] ^ keystream[i];
            }
        } else {
            for i in 0..16 {
                result_block[i] = block[i] ^ keystream[i];
            }
        }
        counter = counter.wrapping_add(1);
        encrypted_data.extend_from_slice(&result_block[..block.len()]);
    }
    Ok(encrypted_data)
}

fn quarter_round(a: &mut u32, b: &mut u32, c: &mut u32, d: &mut u32) {
    // Шаг 1: a += b; d ^= a; d <<<= 16
    *a = a.wrapping_add(*b);
    *d ^= *a;
    *d = d.rotate_left(16);

    // Шаг 2: c += d; b ^= c; b <<<= 12
    *c = c.wrapping_add(*d);
    *b ^= *c;
    *b = b.rotate_left(12);

    // Шаг 3: a += b; d ^= a; d <<<= 8
    *a = a.wrapping_add(*b);
    *d ^= *a;
    *d = d.rotate_left(8);

    // Шаг 4: c += d; b ^= c; b <<<= 7
    *c = c.wrapping_add(*d);
    *b ^= *c;
    *b = b.rotate_left(7);
}

// fn bytes_to_u32_le(bytes: &[u8]) -> [u32; 16] {
//     assert_eq!(bytes.len(), 64);

//     let mut result = [0u32; 16];

//     for i in 0..16 {
//         let chunk = &bytes[i * 4..(i + 1) * 4];

//         // from_le_bytes автоматически учитывает порядок байтов Little-Endian
//         result[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
//     }

//     result
// }

fn u32_to_bytes_le(words: &[u32; 16]) -> Vec<u8> {
    let mut result = Vec::with_capacity(64);
    for &word in words {
        // to_le_bytes превращает u32 в 4 байта (Little-Endian)
        result.extend_from_slice(&word.to_le_bytes());
    }
    result
}

fn create_initial_state(key: &[u8; 32], nonce: &[u8; 12], counter: u32) -> [u32; 16] {
    // 1. Константы "expand 32-byte k" в little-endian
    const CONSTANTS: [u32; 4] = [
        0x61707865, // "expa"
        0x3320646e, // "nd 3"
        0x79622d32, // "2-by"
        0x6b206574, // "te k"
    ];

    let mut state_words = [0u32; 16];

    state_words[0..4].copy_from_slice(&CONSTANTS);

    for i in 0..8 {
        let chunk = &key[i * 4..(i + 1) * 4];
        state_words[4 + i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    state_words[12] = counter;

    for i in 0..3 {
        let chunk = &nonce[i * 4..(i + 1) * 4];
        state_words[13 + i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }

    state_words
}

// please dont look at this shit, i swear it works
fn chacha20_block(state: &mut [u32; 16]) {
    let mut a = state[0];
    let mut b = state[4];
    let mut c = state[8];
    let mut d = state[12];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[0] = a;
    state[4] = b;
    state[8] = c;
    state[12] = d;

    let mut a = state[1];
    let mut b = state[5];
    let mut c = state[9];
    let mut d = state[13];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[1] = a;
    state[5] = b;
    state[9] = c;
    state[13] = d;

    let mut a = state[2];
    let mut b = state[6];
    let mut c = state[10];
    let mut d = state[14];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[2] = a;
    state[6] = b;
    state[10] = c;
    state[14] = d;

    let mut a = state[3];
    let mut b = state[7];
    let mut c = state[11];
    let mut d = state[15];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[3] = a;
    state[7] = b;
    state[11] = c;
    state[15] = d;

    let mut a = state[0];
    let mut b = state[5];
    let mut c = state[10];
    let mut d = state[15];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[0] = a;
    state[5] = b;
    state[10] = c;
    state[15] = d;

    let mut a = state[1];
    let mut b = state[6];
    let mut c = state[11];
    let mut d = state[12];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[1] = a;
    state[6] = b;
    state[11] = c;
    state[12] = d;

    let mut a = state[2];
    let mut b = state[7];
    let mut c = state[8];
    let mut d = state[13];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[2] = a;
    state[7] = b;
    state[8] = c;
    state[13] = d;

    let mut a = state[3];
    let mut b = state[4];
    let mut c = state[9];
    let mut d = state[14];
    quarter_round(&mut a, &mut b, &mut c, &mut d);
    state[3] = a;
    state[4] = b;
    state[9] = c;
    state[14] = d;
}
