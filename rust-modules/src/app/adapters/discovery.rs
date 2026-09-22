//! Per-bridge discovery IO; screen-owned serials reject late results.
use crate::discovery::{Reply, Request};
use crate::ui::machine::InstanceId;
use std::sync::mpsc::{self, Receiver, TryRecvError};
#[derive(Default)]
pub(crate) struct Controller {
    pending: Vec<(InstanceId, Request, Receiver<Reply>)>,
    home: Option<(u32, std::time::Instant, crate::discovery::Page)>,
}
impl Controller {
    pub(crate) fn request(&mut self, who: InstanceId, req: Request, live: bool) -> Option<Reply> {
        let profile=crate::plex::session::current_gen();
        if self.home.as_ref().is_some_and(|h|h.0!=profile){self.home=None;}
        if !live || req.profile != profile {
            return Some(Reply {
                request: req,
                page: None,
            });
        }
        let cached=req.seed.is_none().then(||self.home.as_ref()).flatten();
        let fallback=cached.map(|(_,_,page)|Reply{request:req.clone(),page:Some(page.clone())});
        if cached.is_some_and(|(_,at,_)|at.elapsed()<std::time::Duration::from_secs(15*60)) {return fallback;}
        if self.pending.len()>=4 {return Some(fallback.unwrap_or(Reply{request:req,page:None}));}
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
            fallback
        } else {
            Some(fallback.unwrap_or(Reply {
                request: req,
                page: None,
            }))
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
                    let mut reply=reply.unwrap_or(Reply {
                            request: req,
                            page: None,
                        });
                    if reply.request.seed.is_none(){
                        if let Some(page)=&reply.page {self.home=Some((reply.request.profile,std::time::Instant::now(),page.clone()));}
                        else if let Some((_,_,page))=self.home.as_ref().filter(|h|h.0==reply.request.profile){reply.page=Some(page.clone());}
                    }
                    return Some((who,reply));
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_discover_refresh_keeps_profile_scoped_last_good_home(){
        let _serial=crate::testlock::serial();
        let profile=crate::plex::session::current_gen();
        let req=Request{profile,serial:1,seed:None,name:"Discover".into(),sid:crate::plex::ServerId::UNSET};
        let mut controller=Controller{home:Some((profile,std::time::Instant::now(),crate::discovery::Page{label:"Cached live feed".into(),..Default::default()})),..Default::default()};
        let(tx,rx)=mpsc::channel();tx.send(Reply{request:req.clone(),page:None}).unwrap();
        controller.pending.push((InstanceId(1),req.clone(),rx));
        assert_eq!(controller.poll().unwrap().1.page.unwrap().label,"Cached live feed");
        assert!(controller.request(InstanceId(1),req.clone(),false).unwrap().page.is_none(),"recording/replay never reads live cache");
        let mut retired=req;retired.profile=profile.wrapping_add(1);
        assert!(controller.request(InstanceId(1),retired,true).unwrap().page.is_none(),"another profile never sees the cached page");
    }
}
