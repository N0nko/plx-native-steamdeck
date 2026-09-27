//! Typed catalog effects; cloud IDs are never playable server rating keys.
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub id: String,
    pub profile: u32,
    pub desired: Option<bool>,
}
#[derive(Clone, Debug)]
pub(crate) struct Snapshot {
    pub id: String,
    pub profile: u32,
    pub title: String,
    pub summary: String,
    pub year: i32,
    pub watchlisted: Option<bool>,
    pub busy: bool,
    pub failed: bool,
    pub local: Option<(crate::plex::ServerId, String)>,
    pub poster: Option<(crate::plex::ServerId, String)>,
    pub ratings: Vec<crate::metadata::Rating>,
    pub trailer: Option<crate::plex::catalog::Trailer>,
}
impl Snapshot {
    pub(crate) fn pending(r: &Request) -> Self {
        Self {
            id: r.id.clone(),
            profile: r.profile,
            title: String::new(),
            summary: String::new(),
            year: 0,
            watchlisted: None,
            busy: true,
            failed: false,
            local: None,
            poster: None,
            ratings: Vec::new(),
            trailer: None,
        }
    }
}
