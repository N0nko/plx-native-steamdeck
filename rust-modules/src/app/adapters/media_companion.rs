//! Single-flight, owned resource adapter for the optional paired media companion.
//! Changing the highlighted file retires its observation; late replies never cross identity or
//! screen boundaries. No action is automatic, and mutation requires fresh exact-Part evidence.
use crate::media_companion::{Action, Key, Request, Snapshot};
use crate::ui::machine::InstanceId;
use serde::Deserialize;
use std::io::Read;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pairing {
    host: String,
    port: i32,
    token: String,
    machine_id: String,
    #[serde(default)]
    tls_pin: String,
}
impl Pairing {
    fn valid(&self) -> bool {
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
fn valid_pin(pin: &str) -> bool {
    pin.strip_prefix("sha256//").is_some_and(|p| {
        p.len() == 44
            && p.ends_with('=')
            && p[..43]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
    })
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
pub(crate) enum Health {
    Ready,
    Streaming,
    Preparing,
    Queued,
    Replacing,
    Local,
    Unavailable,
    Requesting,
    #[default]
    Unknown,
}
impl Health {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Streaming => "Streaming",
            Self::Preparing => "Preparing",
            Self::Queued => "Queued",
            Self::Replacing => "Replacing",
            Self::Local => "Local",
            Self::Unavailable => "Unavailable",
            Self::Requesting => "Queuing",
            Self::Unknown => "Unknown",
        }
    }
}
#[derive(Deserialize, Default)]
struct Reply {
    state: Health,
    #[serde(default)]
    can_repair: bool,
    #[serde(default)]
    can_prepare: bool,
    #[serde(default)]
    rating_key: String,
    #[serde(default)]
    machine_id: String,
    #[serde(default)]
    part_id: String,
    #[serde(default)]
    prepare_entry_id: String,
    prepare_state: Option<String>,
    downloaded_bytes: Option<u64>,
    expected_bytes: Option<u64>,
    #[serde(default)]
    progress_reliable: bool,
}
impl Reply {
    fn allows(&self, action: Action) -> bool {
        match action {
            Action::Observe => true,
            Action::Repair => self.can_repair,
            Action::Prepare => {
                self.can_prepare
                    && valid_entry_id(&self.prepare_entry_id)
                    && matches!(self.state, Health::Streaming | Health::Unavailable)
            }
        }
    }
    fn progress_percent(&self) -> Option<u8> {
        if !self.progress_reliable
            || self.state != Health::Preparing
            || !matches!(
                self.prepare_state.as_deref(),
                Some("materializing" | "partial")
            )
        {
            return None;
        }
        match (self.downloaded_bytes, self.expected_bytes) {
            (Some(done), Some(total)) if total > 0 && done <= total => {
                Some(((u128::from(done) * 100 / u128::from(total)).min(99)) as u8)
            }
            _ => None,
        }
    }
    fn label(&self) -> String {
        if self.state == Health::Ready {
            return "Ready".into();
        }
        if matches!(
            self.state,
            Health::Queued | Health::Replacing | Health::Local | Health::Requesting
        ) {
            return self.state.label().into();
        }
        let label = match self.prepare_state.as_deref() {
            Some("queued") => "Queued",
            Some("deferred_capacity") => "Waiting for space",
            Some("materializing" | "partial") => "Preparing",
            Some("failed") => "Prepare failed",
            _ => self.state.label(),
        };
        match self.progress_percent() {
            Some(percent) => format!("{label} {percent}%"),
            None => label.into(),
        }
    }
}
fn valid_entry_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}
fn poll_interval(waiting_since: Option<Instant>, now: Instant, percent: Option<u8>) -> Duration {
    Duration::from_secs(
        match waiting_since.map(|at| now.saturating_duration_since(at).as_secs()) {
            Some(0..=9) => 1,
            Some(10..=29) => 2,
            Some(30..=119) | None => 5,
            Some(_) if percent.is_some_and(|p| p >= 90) => 5,
            Some(_) => 30,
        },
    )
}
fn verified_reply(bytes: &[u8], key: &str, machine: &str) -> Option<Reply> {
    let reply: Reply = serde_json::from_slice(bytes).ok()?;
    (reply.rating_key == key && reply.machine_id == machine).then_some(reply)
}
fn verified_part_reply(bytes: &[u8], key: &str, machine: &str, part: &str) -> Option<Reply> {
    verified_reply(bytes, key, machine).filter(|reply| reply.part_id == part)
}
fn fresh(at: Option<Instant>, now: Instant) -> bool {
    at.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(15))
}
fn status_fresh(at: Option<Instant>, now: Instant) -> bool {
    // Accommodate the 30-second wait cadence plus the bounded 15-second HTTP request.
    at.is_some_and(|at| now.saturating_duration_since(at) < Duration::from_secs(60))
}

fn read_pairing() -> Option<Pairing> {
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct Target {
    who: InstanceId,
    key: Key,
    profile: u32,
}
struct Answer {
    configured: bool,
    reply: Option<Reply>,
}
struct Pending {
    target: Target,
    action: Action,
    rx: Receiver<Answer>,
}

#[derive(Default)]
pub(crate) struct Controller {
    target: Option<Target>,
    pending: Option<Pending>,
    reply: Reply,
    configured: bool,
    at: Option<Instant>,
    last_attempt: Option<Instant>,
    waiting_since: Option<Instant>,
    failed_action: bool,
}
impl Controller {
    /// Keep an already running request in the single-flight slot until it returns. Dropping its
    /// receiver at every focus change would permit an unbounded number of detached workers.
    pub(crate) fn reset(&mut self) {
        self.target = None;
        self.reply = Reply::default();
        self.configured = false;
        self.at = None;
        self.last_attempt = None;
        self.waiting_since = None;
        self.failed_action = false;
    }
    fn view(&self, key: Key, now: Instant) -> Snapshot {
        let busy = self.pending.is_some();
        let matches = self.target.as_ref().is_some_and(|t| t.key == key);
        let current = matches && status_fresh(self.at, now);
        let enabled = matches && !busy && fresh(self.at, now);
        Snapshot {
            key,
            label: if !matches {
                "Checking this file".into()
            } else if self.failed_action {
                "Request not confirmed — refresh status".into()
            } else if busy && !current {
                "Checking this file".into()
            } else if current {
                self.reply.label()
            } else if !self.configured {
                "No valid private pairing for this server".into()
            } else {
                "Status unavailable".into()
            },
            configured: matches && self.configured,
            busy,
            can_repair: enabled && self.reply.allows(Action::Repair),
            can_prepare: enabled && self.reply.allows(Action::Prepare),
        }
    }
    pub(crate) fn request(&mut self, who: InstanceId, request: Request) -> Snapshot {
        let now = Instant::now();
        let target = Target {
            who,
            key: request.key.clone(),
            profile: crate::plex::session::current_gen(),
        };
        if self.target.as_ref() != Some(&target) {
            self.reset();
            self.target = Some(target.clone());
        }
        if self.pending.is_some() {
            return self.view(request.key, now);
        }
        if request.action != Action::Observe
            && !(fresh(self.at, now) && self.reply.allows(request.action))
        {
            return self.view(request.key, now);
        }
        if request.action == Action::Observe
            && self.last_attempt.is_some_and(|last| {
                now.saturating_duration_since(last)
                    < poll_interval(self.waiting_since, now, self.reply.progress_percent())
            })
        {
            return self.view(request.key, now);
        }
        // A sid has meaning only in the live server registry; never resolve against "current".
        let Some(client) = crate::plex::client_for(request.key.sid) else {
            self.configured = false;
            self.at = None;
            return self.view(request.key, now);
        };
        let machine = client.machine_id().to_string();
        let entry = self.reply.prepare_entry_id.clone();
        let key = request.key.clone();
        let action = request.action;
        let (tx, rx) = mpsc::sync_channel(1);
        self.last_attempt = Some(now);
        let profile = target.profile;
        let spawned = crate::task::spawn("media-companion", move || {
            let answer = if profile == crate::plex::session::current_gen() {
                perform(&key, &machine, action, &entry)
            } else {
                Answer {
                    configured: false,
                    reply: None,
                }
            };
            let _ = tx.send(answer);
            crate::ui::idle::invalidate();
        })
        .is_some();
        if spawned {
            self.pending = Some(Pending { target, action, rx });
            if action != Action::Observe {
                self.reply.state = Health::Requesting;
                self.reply.prepare_state = None;
                self.reply.can_prepare = false;
                self.reply.can_repair = false;
                self.waiting_since = Some(now);
                self.at = Some(now);
            }
        } else {
            self.at = None;
            self.failed_action = action != Action::Observe;
        }
        self.view(request.key, now)
    }
    pub(crate) fn poll(&mut self) -> Option<(InstanceId, Snapshot)> {
        let pending = self.pending.as_ref()?;
        let answer = match pending.rx.try_recv() {
            Ok(answer) => answer,
            Err(TryRecvError::Empty) => return None,
            Err(TryRecvError::Disconnected) => Answer {
                configured: self.configured,
                reply: None,
            },
        };
        let finished = self.pending.take()?;
        if self.target.as_ref() != Some(&finished.target)
            || finished.target.profile != crate::plex::session::current_gen()
        {
            return None;
        }
        let now = Instant::now();
        self.configured = answer.configured;
        self.failed_action = answer.reply.is_none() && finished.action != Action::Observe;
        self.at = answer.reply.as_ref().map(|_| now);
        self.reply = answer.reply.unwrap_or_default();
        self.last_attempt = Some(now);
        if matches!(
            self.reply.state,
            Health::Preparing | Health::Queued | Health::Replacing
        ) {
            self.waiting_since.get_or_insert(now);
        } else if self.reply.state != Health::Unknown {
            self.waiting_since = None;
        }
        Some((finished.target.who, self.view(finished.target.key, now)))
    }
}

fn perform(key: &Key, machine: &str, action: Action, entry: &str) -> Answer {
    let Some(p) = read_pairing().filter(|p| p.machine_id == machine) else {
        return Answer {
            configured: false,
            reply: None,
        };
    };
    if !crate::media_companion::valid_id(&key.rk)
        || !crate::media_companion::valid_id(&key.part)
        || (action == Action::Prepare && !valid_entry_id(entry))
    {
        return Answer {
            configured: true,
            reply: None,
        };
    }
    let verb = match action {
        Action::Observe => "media",
        Action::Repair => "repair",
        Action::Prepare => "prepare",
    };
    let mut path = format!(
        "/v1/{verb}?rating_key={}&machine_id={}&part_id={}",
        key.rk, p.machine_id, key.part
    );
    if action == Action::Prepare {
        path.push_str(&format!("&entry_id={entry}"));
    }
    let response = crate::net::request_tls(
        &format!("https://{}:{}{path}", p.host, p.port),
        &[
            "Accept: application/json".into(),
            format!("Authorization: Bearer {}", p.token),
        ],
        if action == Action::Observe {
            "GET"
        } else {
            "POST"
        },
        None,
        crate::net::Timeouts {
            connect_s: 3,
            total_s: 15,
            ..crate::net::API
        },
        false,
        Some(65_536),
        crate::net::Tls::Pinned(&p.tls_pin),
    );
    Answer {
        configured: true,
        reply: response
            .filter(|r| r.status == 200)
            .and_then(|r| verified_part_reply(&r.body, &key.rk, &p.machine_id, &key.part)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replies_must_echo_exact_server_and_item_and_old_bridges_cannot_prepare() {
        let bytes = br#"{"state":"Streaming","rating_key":"42","machine_id":"server"}"#;
        let reply = verified_reply(bytes, "42", "server").unwrap();
        assert!(!reply.allows(Action::Prepare));
        assert!(verified_reply(bytes, "43", "server").is_none());
        assert!(verified_reply(bytes, "42", "other").is_none());
        assert!(verified_reply(br#"{"state":"Ready"}"#, "42", "server").is_none());
    }
    #[test]
    fn initial_polling_is_fast_then_bounded_below_evidence_expiry() {
        let start = Instant::now();
        for (elapsed, seconds) in [
            (0, 1),
            (9, 1),
            (10, 2),
            (29, 2),
            (30, 5),
            (119, 5),
            (120, 30),
            (3600, 30),
        ] {
            let interval = poll_interval(Some(start), start + Duration::from_secs(elapsed), None);
            assert_eq!(interval, Duration::from_secs(seconds));
            assert!(interval < Duration::from_secs(60));
        }
        assert_eq!(poll_interval(None, start, None), Duration::from_secs(5));
        let late = start + Duration::from_secs(300);
        assert_eq!(
            poll_interval(Some(start), late, Some(89)),
            Duration::from_secs(30)
        );
        assert_eq!(
            poll_interval(Some(start), late, Some(90)),
            Duration::from_secs(5)
        );
        assert_eq!(
            poll_interval(Some(start), late, Some(99)),
            Duration::from_secs(5)
        );
        assert!(status_fresh(Some(start), start + Duration::from_secs(45)));
        assert!(!status_fresh(Some(start), start + Duration::from_secs(60)));
        assert!(!fresh(Some(start), start + Duration::from_secs(15)));
    }
    #[test]
    fn progress_uses_only_valid_observed_bytes_and_never_invents_percent() {
        let mut reply = Reply {
            state: Health::Preparing,
            prepare_state: Some("materializing".into()),
            ..Reply::default()
        };
        assert_eq!(reply.label(), "Preparing");
        reply.downloaded_bytes = Some(1073741824);
        reply.expected_bytes = Some(2147483648);
        assert_eq!(
            reply.label(),
            "Preparing",
            "untrusted bytes are not download progress"
        );
        reply.progress_reliable = true;
        assert_eq!(reply.label(), "Preparing 50%");
        reply.downloaded_bytes = Some(2147483648);
        assert_eq!(
            reply.label(),
            "Preparing 99%",
            "whole bytes without Ready must not show 100%"
        );
        reply.downloaded_bytes = Some(u64::MAX);
        reply.expected_bytes = Some(u64::MAX);
        assert_eq!(
            reply.label(),
            "Preparing 99%",
            "percent arithmetic must not overflow"
        );
        reply.expected_bytes = Some(2147483648);
        reply.downloaded_bytes = Some(3 * 1073741824);
        assert_eq!(reply.label(), "Preparing");
        reply.prepare_state = Some("deferred_capacity".into());
        assert_eq!(reply.label(), "Waiting for space");
        reply.prepare_state = Some("failed".into());
        assert_eq!(reply.label(), "Prepare failed");
        reply.state = Health::Replacing;
        assert_eq!(reply.label(), "Replacing");
        reply.state = Health::Ready;
        assert_eq!(reply.label(), "Ready");
    }
    #[test]
    fn pairing_stays_private_and_header_safe() {
        let mut p = Pairing {
            host: "192.168.1.105".into(),
            port: 8093,
            token: "a".repeat(64),
            machine_id: "abc123".into(),
            tls_pin: format!("sha256//{}=", "a".repeat(43)),
        };
        assert!(p.valid());
        p.host = "8.8.8.8".into();
        assert!(!p.valid());
        p.host = "127.0.0.1".into();
        p.token.push_str("\r\nX: bad");
        assert!(!p.valid());
    }

    #[test]
    fn exact_part_is_required_before_capabilities_are_published() {
        let bytes = br#"{"state":"Streaming","rating_key":"42","machine_id":"server","part_id":"123","can_prepare":true,"prepare_entry_id":"entry_1"}"#;
        let reply = verified_part_reply(bytes, "42", "server", "123").unwrap();
        assert!(reply.allows(Action::Prepare));
        assert!(verified_part_reply(bytes, "42", "server", "124").is_none());
        assert!(verified_part_reply(bytes, "43", "server", "123").is_none());
        assert!(verified_part_reply(bytes, "42", "other", "123").is_none());
    }
    #[test]
    fn retiring_a_target_keeps_one_worker_but_discards_its_reply() {
        let key = Key {
            sid: crate::plex::ServerId::default(),
            rk: "42".into(),
            part: "123".into(),
        };
        let target = Target {
            profile: crate::plex::session::current_gen(),
            who: InstanceId(7),
            key: key.clone(),
        };
        let (tx, rx) = mpsc::sync_channel(1);
        let mut c = Controller {
            target: Some(target.clone()),
            pending: Some(Pending {
                target,
                action: Action::Observe,
                rx,
            }),
            ..Controller::default()
        };
        c.reset();
        assert!(
            c.pending.is_some(),
            "focus changes must not fan out detached workers"
        );
        assert!(tx
            .send(Answer {
                configured: true,
                reply: Some(Reply {
                    state: Health::Ready,
                    ..Reply::default()
                })
            })
            .is_ok());
        assert!(
            c.poll().is_none(),
            "an old screen must not receive or publish a late answer"
        );
        assert!(c.pending.is_none());
        assert!(!c.view(key, Instant::now()).can_prepare);
    }
    #[test]
    fn stale_or_pending_observations_never_enable_mutations() {
        let now = Instant::now();
        let key = Key {
            sid: crate::plex::ServerId::default(),
            rk: "42".into(),
            part: "123".into(),
        };
        let mut c = Controller {
            target: Some(Target {
                profile: crate::plex::session::current_gen(),
                who: InstanceId(2),
                key: key.clone(),
            }),
            configured: true,
            at: Some(now),
            reply: Reply {
                state: Health::Streaming,
                can_prepare: true,
                can_repair: true,
                prepare_entry_id: "entry_1".into(),
                ..Reply::default()
            },
            ..Controller::default()
        };
        assert!(c.view(key.clone(), now).can_prepare);
        assert!(
            !c.view(key.clone(), now + Duration::from_secs(15))
                .can_prepare
        );
        let (_tx, rx) = mpsc::sync_channel(1);
        c.pending = Some(Pending {
            target: c.target.clone().unwrap(),
            action: Action::Observe,
            rx,
        });
        assert!(!c.view(key, now).can_repair);
    }
}
