//! Per-bridge discovery IO; screen-owned serials reject late results.
use crate::discovery::{Reply, Request};
use crate::ui::machine::InstanceId;
use std::sync::mpsc::{self, Receiver, TryRecvError};
#[derive(Default)]
pub(crate) struct Controller {
    pending: Vec<(InstanceId, Request, Receiver<Reply>)>,
}
impl Controller {
    pub(crate) fn request(&mut self, who: InstanceId, req: Request, live: bool) -> Option<Reply> {
        if !live || req.profile != crate::plex::session::current_gen() || self.pending.len() >= 4 {
            return Some(Reply {
                request: req,
                page: None,
            });
        }
        let (tx, rx) = mpsc::channel();
        let worker = req.clone();
        if crate::task::spawn_small("catalog discovery", move || {
            let page = crate::discovery::fetch(&worker);
            let _ = tx.send(Reply {
                request: worker,
                page,
            });
            crate::ui::idle::invalidate();
        }) {
            self.pending.push((who, req, rx));
            None
        } else {
            Some(Reply {
                request: req,
                page: None,
            })
        }
    }
    pub(crate) fn poll(&mut self) -> Option<(InstanceId, Reply)> {
        let mut i = 0;
        while i < self.pending.len() {
            match self.pending[i].2.try_recv() {
                Err(TryRecvError::Empty) => i += 1,
                reply => {
                    let (who, req, _) = self.pending.remove(i);
                    if req.profile != crate::plex::session::current_gen() {
                        continue;
                    }
                    return Some((
                        who,
                        reply.unwrap_or(Reply {
                            request: req,
                            page: None,
                        }),
                    ));
                }
            }
        }
        None
    }
}
