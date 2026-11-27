//! Signed app updates: the manifest an admin signs offline with an Ed25519 key, and the checks the app makes
//! before it runs a downloaded setup program. The signature covers the exact manifest bytes; the manifest
//! names the setup program's version, size and BLAKE3 hash.

use std::fmt;
use std::io::Read;

use ring::rand::SystemRandom;
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};

const HEADER: &str = "typeshelf-update 1";

/// `major.minor.patch`, compared numerically (0.10.0 is newer than 0.9.9).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Only plain digits: no leading zeros, signs, pre-release or build suffixes.
    pub fn parse(s: &str) -> Option<Self> {
        let num = |p: &str| (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) && (p == "0" || !p.starts_with('0'))).then(|| p.parse().ok()).flatten();
        let mut parts = s.split('.');
        let v = Version { major: num(parts.next()?)?, minor: num(parts.next()?)?, patch: num(parts.next()?)? };
        parts.next().is_none().then_some(v)
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Which app a release is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    Macos,
}

impl Platform {
    pub fn as_str(self) -> &'static str {
        match self {
            Platform::Windows => "windows",
            Platform::Macos => "macos",
        }
    }

    pub fn from_name(s: &str) -> Option<Platform> {
        match s {
            "windows" => Some(Platform::Windows),
            "macos" => Some(Platform::Macos),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub size: u64,
    /// Lowercase hex BLAKE3 of the setup program.
    pub blake3: String,
    /// Windows manifests carry no platform line (installed apps expect that text); Mac ones add `platform macos`.
    pub platform: Platform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateError {
    BadKey,
    BadSignature,
    BadManifest,
    SizeMismatch,
    HashMismatch,
    Io(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(match self {
            UpdateError::BadKey => "the update key is not valid",
            UpdateError::BadSignature => "the manifest signature does not match the update key",
            UpdateError::BadManifest => "the manifest is not in the expected format",
            UpdateError::SizeMismatch => "the file size does not match the manifest",
            UpdateError::HashMismatch => "the file hash does not match the manifest",
            UpdateError::Io(e) => e,
        })
    }
}

impl std::error::Error for UpdateError {}

impl Manifest {
    pub fn for_bytes(version: Version, platform: Platform, bytes: &[u8]) -> Self {
        Manifest { version, size: bytes.len() as u64, blake3: crate::file_hash(bytes), platform }
    }

    /// The canonical text that gets signed.
    pub fn to_text(&self) -> String {
        let platform = if self.platform == Platform::Macos { "platform macos\n" } else { "" };
        format!("{HEADER}\nversion {}\nsize {}\nblake3 {}\n{platform}", self.version, self.size, self.blake3)
    }

    /// Accepts only the canonical text `to_text` writes.
    pub fn parse(bytes: &[u8]) -> Result<Self, UpdateError> {
        let text = std::str::from_utf8(bytes).map_err(|_| UpdateError::BadManifest)?;
        let mut lines = text.split('\n');
        let mut field = |name: &str| lines.next().and_then(|l| l.strip_prefix(name)).and_then(|v| v.strip_prefix(' '));
        let header_ok = field("typeshelf-update") == Some("1");
        let version = field("version").and_then(Version::parse);
        let size = field("size").and_then(|v| v.bytes().all(|b| b.is_ascii_digit()).then(|| v.parse().ok()).flatten());
        let hash = field("blake3").filter(|h| h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        // A missing line (the blank tail of a Windows manifest) is Windows; only `macos` is spelled out.
        let platform = match field("platform") {
            None => Some(Platform::Windows),
            Some("macos") => Some(Platform::Macos),
            Some(_) => None,
        };
        let (true, Some(version), Some(size), Some(hash), Some(platform)) = (header_ok, version, size, hash, platform) else {
            return Err(UpdateError::BadManifest);
        };
        let m = Manifest { version, size, blake3: hash.to_string(), platform };
        // Byte-for-byte canonical: no extra lines, no CRLF, no leading zeros.
        if m.to_text() != text {
            return Err(UpdateError::BadManifest);
        }
        Ok(m)
    }

    /// True only when this release is strictly newer than `current` (the running app's version).
    pub fn is_newer_than(&self, current: &str) -> bool {
        Version::parse(current).is_some_and(|c| self.version > c)
    }
}

/// The baked-in public key (64 hex characters), or `None` when the build has none or it is malformed.
pub fn public_key(hex: Option<&str>) -> Option<[u8; 32]> {
    from_hex(hex?.trim())?.try_into().ok()
}

/// Checks the signature over the exact `manifest` bytes received, and only then parses them.
pub fn verify_manifest(key: &[u8; 32], manifest: &[u8], signature_hex: &str) -> Result<Manifest, UpdateError> {
    let sig = from_hex(signature_hex.trim()).ok_or(UpdateError::BadSignature)?;
    UnparsedPublicKey::new(&ED25519, key).verify(manifest, &sig).map_err(|_| UpdateError::BadSignature)?;
    Manifest::parse(manifest)
}

/// Reads the whole file and compares its size and BLAKE3 hash with the manifest.
pub fn check_file(manifest: &Manifest, mut file: impl Read) -> Result<(), UpdateError> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(UpdateError::Io(e.to_string())),
        };
        size += n as u64;
        if size > manifest.size {
            return Err(UpdateError::SizeMismatch);
        }
        hasher.update(&buf[..n]);
    }
    if size != manifest.size {
        return Err(UpdateError::SizeMismatch);
    }
    if hasher.finalize().to_hex().as_str() != manifest.blake3 {
        return Err(UpdateError::HashMismatch);
    }
    Ok(())
}

/// A new key pair: `(private key, public key)`, both hex. The private key is a PKCS#8 document.
pub fn generate_key() -> Result<(String, String), UpdateError> {
    let doc = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| UpdateError::BadKey)?;
    let pair = Ed25519KeyPair::from_pkcs8(doc.as_ref()).map_err(|_| UpdateError::BadKey)?;
    Ok((to_hex(doc.as_ref()), to_hex(pair.public_key().as_ref())))
}

/// Signs `manifest` with a private key from `generate_key`; returns `(signature, public key)` as hex.
pub fn sign(private_hex: &str, manifest: &[u8]) -> Result<(String, String), UpdateError> {
    let pkcs8 = from_hex(private_hex.trim()).ok_or(UpdateError::BadKey)?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| UpdateError::BadKey)?;
    Ok((to_hex(pair.sign(manifest).as_ref()), to_hex(pair.public_key().as_ref())))
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys() -> ([u8; 32], String) {
        let (private, public) = generate_key().unwrap();
        (public_key(Some(&public)).unwrap(), private)
    }

    fn release(version: &str, file: &[u8], private: &str) -> (String, String) {
        let text = Manifest::for_bytes(Version::parse(version).unwrap(), Platform::Windows, file).to_text();
        let (sig, _) = sign(private, text.as_bytes()).unwrap();
        (text, sig)
    }

    #[test]
    fn windows_manifest_text_is_unchanged() {
        let m = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Windows, b"x");
        assert_eq!(m.to_text(), format!("typeshelf-update 1\nversion 1.2.3\nsize 1\nblake3 {}\n", m.blake3));
        assert_eq!(Manifest::parse(m.to_text().as_bytes()).unwrap().platform, Platform::Windows);
    }

    #[test]
    fn macos_manifest_adds_one_line_and_round_trips() {
        let m = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Macos, b"x");
        assert_eq!(m.to_text(), format!("typeshelf-update 1\nversion 1.2.3\nsize 1\nblake3 {}\nplatform macos\n", m.blake3));
        assert_eq!(Manifest::parse(m.to_text().as_bytes()), Ok(m));
    }

    #[test]
    fn other_platform_lines_are_refused() {
        let w = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Windows, b"x").to_text();
        for extra in ["platform windows\n", "platform linux\n", "platform macos\nmore\n", "platform  macos\n"] {
            assert_eq!(Manifest::parse(format!("{w}{extra}").as_bytes()), Err(UpdateError::BadManifest), "{extra:?}");
        }
    }

    #[test]
    fn good_release_verifies() {
        let (key, private) = keys();
        let file = b"setup program bytes".repeat(10_000);
        let (text, sig) = release("0.2.0", &file, &private);
        let m = verify_manifest(&key, text.as_bytes(), &sig).unwrap();
        assert_eq!(m.version, Version { major: 0, minor: 2, patch: 0 });
        assert_eq!(m.size, file.len() as u64);
        assert_eq!(check_file(&m, &file[..]), Ok(()));
        assert!(m.is_newer_than("0.1.2"));
    }

    #[test]
    fn tampered_file_is_refused() {
        let (key, private) = keys();
        let file = b"setup program bytes".to_vec();
        let (text, sig) = release("0.2.0", &file, &private);
        let m = verify_manifest(&key, text.as_bytes(), &sig).unwrap();
        let mut changed = file.clone();
        changed[0] ^= 1;
        assert_eq!(check_file(&m, &changed[..]), Err(UpdateError::HashMismatch));
        assert_eq!(check_file(&m, &file[..file.len() - 1]), Err(UpdateError::SizeMismatch));
        let longer = [&file[..], b"x"].concat();
        assert_eq!(check_file(&m, &longer[..]), Err(UpdateError::SizeMismatch));
    }

    #[test]
    fn tampered_manifest_is_refused() {
        let (key, private) = keys();
        let (text, sig) = release("0.2.0", b"file", &private);
        // A higher version on the same signature, and the same bytes with one flipped.
        let bumped = text.replace("version 0.2.0", "version 9.2.0");
        assert_eq!(verify_manifest(&key, bumped.as_bytes(), &sig), Err(UpdateError::BadSignature));
        let mut flipped = text.clone().into_bytes();
        flipped[30] ^= 1;
        assert_eq!(verify_manifest(&key, &flipped, &sig), Err(UpdateError::BadSignature));
        // Not a signature at all.
        assert_eq!(verify_manifest(&key, text.as_bytes(), "zz"), Err(UpdateError::BadSignature));
        assert_eq!(verify_manifest(&key, text.as_bytes(), ""), Err(UpdateError::BadSignature));
    }

    #[test]
    fn wrong_key_is_refused() {
        let (_, private) = keys();
        let (other, _) = keys();
        let (text, sig) = release("0.2.0", b"file", &private);
        assert_eq!(verify_manifest(&other, text.as_bytes(), &sig), Err(UpdateError::BadSignature));
    }

    #[test]
    fn older_or_same_version_is_not_newer() {
        let (key, private) = keys();
        let (text, sig) = release("0.1.1", b"file", &private);
        let m = verify_manifest(&key, text.as_bytes(), &sig).unwrap();
        assert!(!m.is_newer_than("0.1.2"));
        assert!(!m.is_newer_than("0.1.1"));
        assert!(m.is_newer_than("0.1.0"));
        // Numeric, not string, comparison.
        let ten = Manifest { version: Version::parse("0.10.0").unwrap(), size: 0, blake3: String::new(), platform: Platform::Windows };
        assert!(ten.is_newer_than("0.9.9"));
        assert!(!Manifest { version: Version::parse("0.9.9").unwrap(), ..ten.clone() }.is_newer_than("0.10.0"));
        // An unreadable running version never installs anything.
        assert!(!ten.is_newer_than("dev"));
    }

    #[test]
    fn only_the_canonical_manifest_parses() {
        let (key, private) = keys();
        let m = Manifest::for_bytes(Version::parse("1.2.3").unwrap(), Platform::Windows, b"x");
        assert_eq!(Manifest::parse(m.to_text().as_bytes()), Ok(m.clone()));
        let good = m.to_text();
        for bad in [
            good.replace("1.2.3", "01.2.3"),
            good.replace("1.2.3", "1.2"),
            good.replace("1.2.3", "1.2.3-beta"),
            good.replace("size 1", "size +1"),
            good.to_uppercase(),
            format!("{good}extra 1\n"),
            good.trim_end().to_string(),
            good.replace('\n', "\r\n"),
        ] {
            assert_eq!(Manifest::parse(bad.as_bytes()), Err(UpdateError::BadManifest), "{bad:?}");
            // Correctly signed but malformed is still refused.
            let (sig, _) = sign(&private, bad.as_bytes()).unwrap();
            assert_eq!(verify_manifest(&key, bad.as_bytes(), &sig), Err(UpdateError::BadManifest), "{bad:?}");
        }
    }

    #[test]
    fn a_build_without_a_valid_key_has_none() {
        assert_eq!(public_key(None), None);
        assert_eq!(public_key(Some("")), None);
        assert_eq!(public_key(Some("not hex")), None);
        assert_eq!(public_key(Some(&"ab".repeat(31))), None);
        assert!(public_key(Some(&"AB".repeat(32))).is_some());
    }
}
