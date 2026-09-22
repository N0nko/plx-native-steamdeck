//! Bridge-owned bounded catalog workers. Retired profiles cannot land or authorize a write.
use crate::catalog_action::{Request, Snapshot};
use crate::ui::machine::InstanceId;
use std::sync::mpsc::{self, Receiver, TryRecvError};
#[derive(Default)]
pub(crate) struct Controller {
    pending: Vec<(InstanceId, Request, Receiver<Snapshot>)>,
}
impl Controller {
    pub(crate) fn request(&mut self, who: InstanceId, request: Request, live: bool) -> Snapshot {
        let mut result = Snapshot::pending(&request);
        let profile = crate::plex::session::current_gen();
        let client = crate::plex::catalog::active_client();
        if !live
            || profile != request.profile
            || !crate::plex::catalog::valid_id(&request.id)
            || client.is_none()
            || self.pending.len() >= 4
            || self.pending.iter().any(|(id, _, _)| *id == who)
        {
            result.busy = false;
            result.failed = true;
            return result;
        }
        let client = client.unwrap();
        let worker = request.clone();
        let (tx, rx) = mpsc::channel();
        let started = crate::task::spawn_small("catalog action", move || {
            let mut reply = Snapshot::pending(&worker);
            reply.busy = false;
            if worker.profile == crate::plex::session::current_gen() {
                reply.watchlisted = match worker.desired {
                    Some(desired) => client.set_watchlisted(&worker.id, desired),
                    None => client.watchlisted(&worker.id),
                };
                if let Some(meta) = client.catalog_detail(&worker.id) {
                    for sid in crate::plex::server_ids() {
                        if worker.profile != crate::plex::session::current_gen() { break; }
                        let Some(server) = crate::plex::client_for(sid) else { continue };
                        let Some(found) = server.find_by_guid(&meta.guid) else { continue };
                        if let Some(rk) = exact_local(&meta.guid, &found.metadata) {
                            reply.local = Some((sid, rk));
                            break;
                        }
                    }
                    reply.title = meta.title;
                    reply.summary = meta.summary;
                    reply.year = meta.year as i32;
                }
            }
            reply.failed = reply.watchlisted.is_none();
            let _ = tx.send(reply);
            crate::ui::idle::invalidate();
        });
        if started {
            self.pending.push((who, request, rx));
        } else {
            result.busy = false;
            result.failed = true;
        }
        result
    }
    pub(crate) fn poll(&mut self) -> Option<(InstanceId, Snapshot)> {
        let mut i = 0;
        while i < self.pending.len() {
            match self.pending[i].2.try_recv() {
                Err(TryRecvError::Empty) => i += 1,
                answer => {
                    let (who, req, _) = self.pending.remove(i);
                    if req.profile != crate::plex::session::current_gen() {
                        continue;
                    }
                    let reply = answer.unwrap_or_else(|_| {
                        let mut s = Snapshot::pending(&req);
                        s.busy = false;
                        s.failed = true;
                        s
                    });
                    return Some((who, reply));
                }
            }
        }
        None
    }
}

/// A title match or a provider ID is never a playable library identity.
fn exact_local(guid: &str, rows: &[crate::plex::Metadata]) -> Option<String> {
    let kind = guid.strip_prefix("plex://")?.split_once('/')?.0;
    if !matches!(kind, "movie" | "show") { return None; }
    rows.iter().find(|m| m.guid == guid && m.kind == kind && !m.rating_key.is_empty())
        .map(|m| m.rating_key.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn library_match_requires_exact_guid_kind_and_server_key() {
        let guid="plex://movie/0123456789abcdef01234567";
        let mut m=crate::plex::Metadata{guid:guid.into(),kind:"show".into(),rating_key:"42".into(),..Default::default()};
        assert!(exact_local(guid,std::slice::from_ref(&m)).is_none());
        m.kind="movie".into();assert_eq!(exact_local(guid,std::slice::from_ref(&m)),Some("42".into()));
        m.guid="plex://movie/1123456789abcdef01234567".into();assert!(exact_local(guid,std::slice::from_ref(&m)).is_none());
        m.guid=guid.into();m.rating_key.clear();assert!(exact_local(guid,&[m]).is_none());
    }
}
