//! Create a connection to the rkpvm.

use anyhow::{anyhow, ensure, Result};
use openssl::derive::Deriver;
use openssl::hkdf::hkdf;
use openssl::md::Md;
use openssl::pkey::{HasPrivate, HasPublic, Id, PKey, PKeyRef, Private};
use openssl::sign::Signer;
use openssl::symm::{decrypt_aead, encrypt_aead, Cipher};

/// Derives a cipher key, shared between the initiator and responder.
///
/// Firstly, an ECDH between the ephemeral keys of the initiator and responder
/// is used to calculate a shared secret. That value is then used as the input
/// key material to an HKDF, which produces the final cipher key.
fn derive_cipher_key<T, U>(key: &PKeyRef<T>, peer: &PKeyRef<U>) -> Result<[u8; 32]>
where
    T: HasPrivate,
    U: HasPublic,
{
    let mut deriver = Deriver::new(key)?;
    deriver.set_peer(peer)?;
    let mut ikm = [0; 32];
    deriver.derive(&mut ikm)?;
    let mut cipher_key = [0; 32];
    // TODO: pick some useful info?
    hkdf(&mut cipher_key, Md::sha256(), &ikm, &[], &[])?;
    Ok(cipher_key)
}

/// Produces a response for the corresponding initiator.
///
/// An ephemeral key is generated and used to derive a shared cipher key with
/// the initiator, based on their public key. The message is encrypted with the
/// cipher key and returned along with the responder's ephemeral public key.
pub fn respond(initiator_public_key_raw: &[u8], plaintext: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let initiator_public_key =
        PKey::public_key_from_raw_bytes(initiator_public_key_raw, Id::X25519)?;
    let ephemeral_key_pair = PKey::generate_x25519()?;
    let ephemeral_public_key_raw = ephemeral_key_pair.raw_public_key()?;
    let mut tag = [0; 16];
    let iv = [0; 12]; // TODO: random?
    let cipher_key = derive_cipher_key(&ephemeral_key_pair, &initiator_public_key)?;
    let mut ciphertext =
        encrypt_aead(Cipher::aes_256_gcm(), &cipher_key, Some(&iv), &[], plaintext, &mut tag)?;
    ciphertext.extend_from_slice(&tag);
    Ok((ephemeral_public_key_raw, ciphertext))
}

/// Initiator for the exchange of protected data.
pub struct Initiator {
    ephemeral_key_pair: PKey<Private>,
}

impl Initiator {
    /// Creates a new initiator with an ephemeral key.
    pub fn new() -> Result<Self> {
        Ok(Self { ephemeral_key_pair: PKey::generate_x25519()? })
    }

    /// Returns the initiator's ephemeral public key, signed with the provided
    /// signing key. This signature can be used to authenticate the ephemeral
    /// public key.
    pub fn signed_public_key<T>(&self, signing_key: &PKeyRef<T>) -> Result<(Vec<u8>, Vec<u8>)>
    where
        T: HasPrivate,
    {
        let ephemeral_public_key_raw = self.ephemeral_key_pair.raw_public_key()?;
        let mut signer = Signer::new_without_digest(signing_key)?;
        let mut signature = [0; 64];
        ensure!(signer.sign_oneshot(&mut signature, &ephemeral_public_key_raw)? == signature.len());
        Ok((ephemeral_public_key_raw, signature.to_vec()))
    }

    /// Receives a responce from the corresponding responder.
    ///
    /// The shared cipher key is derived based on the responder's ephemeral
    /// public key and is used to decrypt the message.
    pub fn receive(self, responder_public_key_raw: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 16 {
            return Err(anyhow!("ciphertext can't contain tag"));
        }
        let (ciphertext, tag) = ciphertext.split_at(ciphertext.len() - 16);
        let iv = [0; 12]; // TODO: random?
        let responder_public_key =
            PKey::public_key_from_raw_bytes(responder_public_key_raw, Id::X25519)?;
        let cipher_key = derive_cipher_key(&self.ephemeral_key_pair, &responder_public_key)?;
        let plaintext =
            decrypt_aead(Cipher::aes_256_gcm(), &cipher_key, Some(&iv), &[], ciphertext, tag)?;
        Ok(plaintext)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_connection() -> Result<()> {
        let initiator = Initiator::new()?;
        let initiator_attestation_key = PKey::generate_ed25519()?;
        let data = b"Secret message";
        let (responder_public_key_raw, ciphertext) =
            respond(&initiator.signed_public_key(&initiator_attestation_key)?.0, data)?;
        let plaintext = initiator.receive(&responder_public_key_raw, &ciphertext)?;
        assert_eq!(&plaintext, data);
        Ok(())
    }
}
