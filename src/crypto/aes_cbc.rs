use aes::cipher::{BlockCipherEncrypt, BlockCipherDecrypt};

pub fn encrypt(plaintext: &[u8], cipher: &aes::Aes128) -> Vec<u8> {
    let iv: [u8; 16] = rand::random();

    let mut prev_block = iv;
    let mut ciphertext = Vec::with_capacity(16 + plaintext.len().next_multiple_of(16));

    ciphertext.extend_from_slice(&prev_block);

    for chunk in plaintext.chunks(16) {
        // Pad block to be 16 bytes
        let mut block = [0; 16];
        block[..chunk.len()].copy_from_slice(chunk);

        // XOR with previous block
        block.iter_mut().zip(&prev_block).for_each(|(b, p)| *b ^= *p);

        // AES encryption
        let mut block_array = aes::cipher::Array::from(block);
        cipher.encrypt_block(&mut block_array);

        prev_block = block_array.into();
        ciphertext.extend_from_slice(&prev_block);
    }

    ciphertext
}

pub fn decrypt(ciphertext: &[u8], cipher: &aes::Aes128) -> Vec<u8> {
    let chunks: Vec<&[u8]> = ciphertext.chunks(16).collect();
    let mut plaintext = Vec::with_capacity(chunks.len() - 1);

    for i in 1..chunks.len() {
        let mut block = [0; 16];
        block.copy_from_slice(chunks[i]);

        // AES decryption
        let mut block_array = aes::cipher::Array::from(block);
        cipher.decrypt_block(&mut block_array);

        block = block_array.into();

        // XOR with previous block
        let prev_block = chunks[i - 1];

        for (a, b) in block.iter_mut().zip(prev_block) {
            *a ^= *b;
        }

        plaintext.push(block.to_vec());
    }

    plaintext.into_iter().flatten().collect()
}