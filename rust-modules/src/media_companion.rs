//! Typed, secret-free messages for the optional paired media companion.
//! Resources belong to app::adapters::media_companion, not a process-global UI controller.
use crate::plex::ServerId;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    pub(crate) sid: ServerId,
    pub(crate) rk: String,
    pub(crate) part: String,
}
impl Key {
    pub(crate) fn new(sid: ServerId, rk: &str, path: &str) -> Option<Self> {
        let part = path.strip_prefix("/library/parts/")?.split_once('/')?.0;
        (valid_id(rk) && valid_id(part)).then(|| Self {
            sid,
            rk: rk.into(),
            part: part.into(),
        })
    }
}
pub(crate) fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 20
        && value.bytes().all(|b| b.is_ascii_digit())
        && value.parse::<u64>().is_ok_and(|n| n > 0)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Action {
    Observe,
    Repair,
    Prepare,
}
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub(crate) key: Key,
    pub(crate) action: Action,
}

/// Only closed status text and capabilities reach the screen; no pairing token, remote reason,
/// address, or preparation entry id is published, recorded, or logged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) key: Key,
    pub(crate) label: String,
    pub(crate) configured: bool,
    pub(crate) busy: bool,
    pub(crate) can_repair: bool,
    pub(crate) can_prepare: bool,
}
impl Snapshot {
    pub(crate) fn unavailable(key: Key, label: &str) -> Self {
        Self {
            key,
            label: label.into(),
            configured: false,
            busy: false,
            can_repair: false,
            can_prepare: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_require_exact_local_part_paths_and_positive_ids() {
        let sid = ServerId::default();
        assert_eq!(
            Key::new(sid, "42", "/library/parts/123/stamp/file.mkv")
                .unwrap()
                .part,
            "123"
        );
        for path in [
            "",
            "https://other/library/parts/123/a",
            "/library/parts/0/a",
            "/library/parts/12&other=1/a",
            "/library/parts/123",
        ] {
            assert!(Key::new(sid, "42", path).is_none());
        }
        for rk in ["", "0", "42&action=repair", "18446744073709551616"] {
            assert!(Key::new(sid, rk, "/library/parts/123/a").is_none());
        }
    }
}
