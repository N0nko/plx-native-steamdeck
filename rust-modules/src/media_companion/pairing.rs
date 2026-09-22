//! Private, pinned companion credentials shared by background adapters.
use serde::Deserialize;
use std::io::Read;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pairing {
    pub(crate) host: String,
    pub(crate) port: i32,
    pub(crate) token: String,
    pub(crate) machine_id: String,
    #[serde(default)]
    pub(crate) tls_pin: String,
}
impl Pairing {
    pub(crate) fn valid(&self) -> bool {
        let local = self
            .host
            .parse::<std::net::Ipv4Addr>()
            .is_ok_and(|ip| ip.is_private() || ip.is_loopback());
        local
            && (1..=65535).contains(&self.port)
            && self.token.len() >= 48
            && self.token.bytes().all(|b| b.is_ascii_hexdigit())
            && !self.machine_id.is_empty()
            && self.machine_id.bytes().all(|b| b.is_ascii_alphanumeric())
            && valid_pin(&self.tls_pin)
    }
}
pub(crate) fn valid_pin(pin: &str) -> bool {
    pin.strip_prefix("sha256//").is_some_and(|p| {
        p.len() == 44
            && p.ends_with('=')
            && p[..43]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    })
}

pub(crate) fn read_pairing() -> Option<Pairing> {
    // Preserve the original fork's path; also accept the new persistent state root. Never copy
    // credentials into the source tree or convert a bad pairing into an unpinned connection.
    let paths = [
        crate::paths::persistent_state_root().join("media-companion.json"),
        crate::paths::in_app_dir("state").join("media-companion.json"),
    ];
    for path in paths {
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
        }
        let Ok(file) = options.open(path) else {
            continue;
        };
        let Ok(meta) = file.metadata() else {
            continue;
        };
        if !meta.is_file() || meta.len() > 16_384 {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if meta.permissions().mode() & 0o077 != 0 {
                continue;
            }
        }
        let mut bytes = Vec::new();
        if file.take(16_385).read_to_end(&mut bytes).is_err() || bytes.len() > 16_384 {
            continue;
        }
        let Ok(pairing) = serde_json::from_slice::<Pairing>(&bytes) else {
            continue;
        };
        if pairing.valid() {
            return Some(pairing);
        }
    }
    None
}

