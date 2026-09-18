//! SHA-256 through CNG (`bcrypt.dll`), the system's own implementation.
//!
//! A hashing crate would be smaller to call, but the rule is Microsoft
//! libraries only, and the file being hashed is the one the program is
//! about to run: the fewer hands between the download and the verdict, the
//! better. One algorithm handle per call; the program hashes one file per
//! update and nothing else.

use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE, BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS,
    BCRYPT_SHA256_ALGORITHM, BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash,
    BCryptFinishHash, BCryptHashData, BCryptOpenAlgorithmProvider,
};
use windows::core::PCWSTR;

/// The SHA-256 of `path`, lower-case hex, read in 64 KiB pieces.
pub fn sha256_of(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("cannot open `{}` to verify it", path.display()))?;
    let mut hasher = Sha256::new()?;
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut buffer)
            .with_context(|| format!("cannot read `{}` to verify it", path.display()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read])?;
    }
    hasher.finish()
}

/// The SHA-256 of a byte string, for the tests and nothing else so far.
#[cfg(test)]
fn sha256_of_bytes(bytes: &[u8]) -> Result<String> {
    let mut hasher = Sha256::new()?;
    hasher.update(bytes)?;
    hasher.finish()
}

/// An open algorithm and hash object, closed in order on drop.
struct Sha256 {
    algorithm: BCRYPT_ALG_HANDLE,
    hash: BCRYPT_HASH_HANDLE,
}

impl Sha256 {
    fn new() -> Result<Self> {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        // SAFETY: `algorithm` is a valid out-pointer for the call; the
        // algorithm name is a static NUL-terminated string; no implementation
        // is named, so the system's default provider answers. The handle is
        // closed in `drop`.
        let status = unsafe {
            BCryptOpenAlgorithmProvider(
                &mut algorithm,
                BCRYPT_SHA256_ALGORITHM,
                PCWSTR::null(),
                BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0),
            )
        };
        if status.is_err() {
            return Err(anyhow!("BCryptOpenAlgorithmProvider failed: {status:?}"));
        }
        let mut hash = BCRYPT_HASH_HANDLE::default();
        // SAFETY: `algorithm` was just opened; `hash` is a valid out-pointer;
        // no caller-supplied hash object, so CNG allocates its own, freed by
        // BCryptDestroyHash in `drop`; no secret, this is a plain hash.
        let status = unsafe { BCryptCreateHash(algorithm, &mut hash, None, None, 0) };
        if status.is_err() {
            // SAFETY: the algorithm handle is open and closed exactly once here.
            unsafe { _ = BCryptCloseAlgorithmProvider(algorithm, 0) };
            return Err(anyhow!("BCryptCreateHash failed: {status:?}"));
        }
        Ok(Self { algorithm, hash })
    }

    fn update(&mut self, bytes: &[u8]) -> Result<()> {
        // SAFETY: the hash handle is open; the slice is read for its length
        // and not kept.
        let status = unsafe { BCryptHashData(self.hash, bytes, 0) };
        if status.is_err() {
            return Err(anyhow!("BCryptHashData failed: {status:?}"));
        }
        Ok(())
    }

    fn finish(self) -> Result<String> {
        let mut digest = [0u8; 32];
        // SAFETY: the hash handle is open and `digest` is exactly the SHA-256
        // output length, which is what the call writes.
        let status = unsafe { BCryptFinishHash(self.hash, &mut digest, 0) };
        if status.is_err() {
            return Err(anyhow!("BCryptFinishHash failed: {status:?}"));
        }
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

impl Drop for Sha256 {
    fn drop(&mut self) {
        // SAFETY: both handles were opened in `new`, the hash before the
        // algorithm is closed as CNG requires, each destroyed once.
        unsafe {
            _ = BCryptDestroyHash(self.hash);
            _ = BCryptCloseAlgorithmProvider(self.algorithm, 0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_system_hashes_the_way_the_release_workflow_does() {
        // FIPS 180-4's own test vectors, which `sha256sum` and the workflow
        // agree with.
        assert_eq!(
            sha256_of_bytes(b"abc").unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_of_bytes(b"").unwrap(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_file_is_hashed_in_pieces_to_the_same_result() {
        let dir = std::env::temp_dir().join(format!("gme-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("big.bin");
        // Larger than one read, so the loop runs more than once.
        let bytes: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(&path, &bytes).unwrap();
        assert_eq!(sha256_of(&path).unwrap(), sha256_of_bytes(&bytes).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
