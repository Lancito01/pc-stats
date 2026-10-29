// Signs updates with a P-256 key kept by Windows (CNG). The key is created on this PC and
// marked non-exportable: it never exists as a file, can't be copied off the machine, and is
// tied to this Windows account. The site only ever gets the public half.

use windows_sys::Win32::Security::Cryptography::{
    BCryptHash, NCryptCreatePersistedKey, NCryptExportKey, NCryptFinalizeKey, NCryptFreeObject, NCryptOpenKey,
    NCryptOpenStorageProvider, NCryptSetProperty, NCryptSignHash, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDSA_P256_ALGORITHM,
    BCRYPT_SHA256_ALG_HANDLE, MS_KEY_STORAGE_PROVIDER, NCRYPT_EXPORT_POLICY_PROPERTY, NCRYPT_KEY_HANDLE,
    NCRYPT_PROV_HANDLE,
};

use crate::util::{base64, base64url, wide};

const KEY_NAME: &str = "pc-stats";
// The same prefix the site checks; it keeps these signatures from meaning anything elsewhere
const CONTEXT: &str = "pc-stats/v1";

pub struct Signer {
    key: NCRYPT_KEY_HANDLE,
}

impl Signer {
    /// Opens this account's key, creating it on first run.
    pub fn open_or_create() -> Result<Signer, String> {
        Self::open(Some(KEY_NAME))
    }

    /// A throwaway key that's never saved, for --self-test.
    pub fn ephemeral() -> Result<Signer, String> {
        Self::open(None)
    }

    fn open(name: Option<&str>) -> Result<Signer, String> {
        unsafe {
            let mut provider: NCRYPT_PROV_HANDLE = 0;
            check(NCryptOpenStorageProvider(&mut provider, MS_KEY_STORAGE_PROVIDER, 0), "open the key store")?;
            let name_w = name.map(wide);
            let name_ptr = name_w.as_ref().map_or(std::ptr::null(), |n| n.as_ptr());

            let mut key: NCRYPT_KEY_HANDLE = 0;
            let opened = name.is_some() && NCryptOpenKey(provider, &mut key, name_ptr, 0, 0) == 0;
            if !opened {
                let result = (|| {
                    check(
                        NCryptCreatePersistedKey(provider, &mut key, BCRYPT_ECDSA_P256_ALGORITHM, name_ptr, 0, 0),
                        "create the key",
                    )?;
                    let no_export: u32 = 0;
                    check(
                        NCryptSetProperty(key, NCRYPT_EXPORT_POLICY_PROPERTY, &no_export as *const u32 as *const u8, 4, 0),
                        "lock the key to this PC",
                    )?;
                    check(NCryptFinalizeKey(key, 0), "save the key")
                })();
                if let Err(error) = result {
                    NCryptFreeObject(provider);
                    return Err(error);
                }
            }
            NCryptFreeObject(provider);
            Ok(Signer { key })
        }
    }

    /// Base64 signature (IEEE P1363, r || s) over the context, the site's host, the timestamp
    /// and the body. Signing the host means an update meant for one site is useless on another.
    pub fn sign(&self, host: &str, timestamp: u64, body: &str) -> Result<String, String> {
        let message = format!("{CONTEXT}\n{host}\n{timestamp}\n{body}");
        let mut hash = [0u8; 32];
        let mut signature = [0u8; 64];
        let mut written = 0u32;
        unsafe {
            check(
                BCryptHash(
                    BCRYPT_SHA256_ALG_HANDLE,
                    std::ptr::null(),
                    0,
                    message.as_ptr(),
                    message.len() as u32,
                    hash.as_mut_ptr(),
                    hash.len() as u32,
                ),
                "hash the update",
            )?;
            check(
                NCryptSignHash(
                    self.key,
                    std::ptr::null(),
                    hash.as_ptr(),
                    hash.len() as u32,
                    signature.as_mut_ptr(),
                    signature.len() as u32,
                    &mut written,
                    0,
                ),
                "sign the update",
            )?;
        }
        Ok(base64(&signature[..written as usize]))
    }

    /// The public key as a JWK, which is what STATS_PUBLIC_KEY on Vercel expects.
    pub fn public_jwk(&self) -> Result<String, String> {
        // BCRYPT_ECCKEY_BLOB: magic (4 bytes), key length (4 bytes), then X and Y
        let mut blob = [0u8; 8 + 64];
        let mut written = 0u32;
        unsafe {
            check(
                NCryptExportKey(
                    self.key,
                    0,
                    BCRYPT_ECCPUBLIC_BLOB,
                    std::ptr::null(),
                    blob.as_mut_ptr(),
                    blob.len() as u32,
                    &mut written,
                    0,
                ),
                "read the public key",
            )?;
        }
        let (x, y) = (&blob[8..40], &blob[40..72]);
        Ok(format!(r#"{{"kty":"EC","crv":"P-256","x":"{}","y":"{}"}}"#, base64url(x), base64url(y)))
    }
}

impl Drop for Signer {
    fn drop(&mut self) {
        unsafe { NCryptFreeObject(self.key) };
    }
}

fn check(status: i32, what: &str) -> Result<(), String> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!("couldn't {what} (0x{:08X})", status as u32))
    }
}
