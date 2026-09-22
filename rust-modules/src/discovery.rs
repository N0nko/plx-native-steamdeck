//! Profile-scoped discovery messages and portable result projections.
use crate::{
    plex::{catalog::DiscoverySeed, ServerId},
    search::{Item, Kind, Shelf, TagHit},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Request {
    pub profile: u32,
    pub serial: u32,
    pub seed: Option<DiscoverySeed>,
    pub name: String,
    pub sid: ServerId,
}
#[derive(Clone, Default)]
pub(crate) struct Page {
    pub label: String,
    pub rows: Vec<Shelf>,
    pub headings: Vec<String>,
}
impl std::fmt::Debug for Page {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscoveryPage")
            .field("label", &self.label)
            .field("rows", &self.rows.len())
            .finish()
    }
}
#[derive(Clone, Debug)]
pub(crate) struct Reply {
    pub request: Request,
    pub page: Option<Page>,
}
pub(crate) fn seed(item: &Item) -> Option<DiscoverySeed> {
    let guid = match item {
        Item::Media(m) | Item::Catalog(m) => &m.guid,
        Item::Tag(t) => &t.tag_key,
    };
    let (kind, id) = guid.strip_prefix("plex://")?.split_once('/')?;
    if !crate::plex::catalog::valid_id(id) {
        return None;
    }
    match kind {
        "movie" | "show" => Some(DiscoverySeed::Title { id: id.into() }),
        "person" => Some(DiscoverySeed::Person { id: id.into() }),
        _ => None,
    }
}
impl Page {
    pub(crate) fn search(rows: &[Shelf]) -> Self {
        Self {
            label: "Search results".into(),
            rows: rows.to_vec(),
            headings: rows.iter().map(|s| s.kind.title().into()).collect(),
        }
    }
    pub(crate) fn filter(&self, filter: usize) -> Self {
        let mut out = Self {
            label: self.label.clone(),
            ..Default::default()
        };
        for (row, title) in self.rows.iter().zip(&self.headings) {
            if filter == 0
                || filter == 1 && row.kind == Kind::Movie
                || filter == 2 && row.kind == Kind::Show
            {
                out.rows.push(row.clone());
                out.headings.push(title.clone());
            }
        }
        out
    }
    pub(crate) fn upgrade(&mut self, known: &[Shelf]) {
        for row in &mut self.rows {
            for item in &mut row.items {
                if let Item::Catalog(m) = item {
                    if let Some(local)=known.iter().flat_map(|s|&s.items).find(|it|matches!(it,Item::Media(local) if !local.guid.is_empty() && local.guid==m.guid)){*item=local.clone();}
                }
            }
        }
    }
    pub(crate) fn write(&self, c: &mut crate::ui::machine::Canon) {
        c.str(&self.label).seq(self.rows.len());
        for (row, title) in self.rows.iter().zip(&self.headings) {
            c.str(title).str(row.kind.title()).seq(row.items.len());
            for it in &row.items {
                c.str(it.title()).u32(it.sid().raw() as u32);
                match it {
                    Item::Media(m) => {
                        c.u32(0).str(&m.rk).str(&m.guid).bool(m.watched);
                    }
                    Item::Catalog(m) => {
                        c.u32(1).str(&m.rk).str(&m.guid);
                    }
                    Item::Tag(t) => {
                        c.u32(2).str(&t.tag_key).str(&t.id);
                    }
                }
            }
        }
    }
}
pub(crate) fn fetch(req: &Request) -> Option<Page> {
    if req.profile != crate::plex::session::current_gen() {
        return None;
    }
    let mut page = Page {
        label: req.name.clone(),
        ..Default::default()
    };
    if let Some(seed) = &req.seed {
        let raw = crate::plex::catalog::active_client()?.discover_related(seed)?;
        for kind in ["movie", "show"] {
            let items = raw
                .similar
                .iter()
                .filter(|m| {
                    m.kind == kind
                        && crate::plex::catalog::valid_id(&m.rating_key)
                        && m.guid == format!("plex://{kind}/{}", m.rating_key)
                })
                .take(24)
                .map(|m| Item::Catalog(crate::pms::parse_item(m, req.sid)))
                .collect::<Vec<_>>();
            if !items.is_empty() {
                page.rows.push(Shelf {
                    kind: if kind == "show" {
                        Kind::Show
                    } else {
                        Kind::Movie
                    },
                    items,
                });
                page.headings.push(match seed {
                    DiscoverySeed::Title { .. } => format!("More like {} · {}", req.name, kind),
                    DiscoverySeed::Person { .. } => format!("With {} · {}", req.name, kind),
                });
            }
        }
        let items = raw
            .people
            .into_iter()
            .filter(|p| crate::plex::catalog::valid_id(&p.id))
            .take(16)
            .map(|p| {
                Item::Tag(TagHit {
                    sid: req.sid,
                    name: p.name,
                    tag_key: format!("plex://person/{}", p.id),
                    thumb: p.thumb,
                    ..Default::default()
                })
            })
            .collect::<Vec<_>>();
        if !items.is_empty() {
            page.rows.push(Shelf {
                kind: Kind::Person,
                items,
            });
            page.headings.push(format!("Cast of {}", req.name));
        }
    } else {
        page.label = "Discover".into();
        for hub in crate::plex::discovery_home::discovery_home(req.profile)? {
            page.headings.push(hub.title);
            page.rows.push(Shelf {
                kind: if hub.kind == "show" {
                    Kind::Show
                } else {
                    Kind::Movie
                },
                items: hub
                    .items
                    .iter()
                    .map(|m| Item::Catalog(crate::pms::parse_item(m, req.sid)))
                    .collect(),
            });
        }
    }
    (req.profile == crate::plex::session::current_gen()).then_some(page)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_rating_keys_cannot_be_discovery_seeds() {
        let mut m = crate::pms::PmsMovie {
            rk: "123".into(),
            ..Default::default()
        };
        assert!(seed(&Item::Media(m.clone())).is_none());
        m.guid = "plex://movie/0123456789abcdef01234567".into();
        assert!(matches!(
            seed(&Item::Media(m)),
            Some(DiscoverySeed::Title { .. })
        ));
    }
    #[test]
    fn filtering_preserves_feed_names() {
        let p = Page {
            label: "Discover".into(),
            rows: vec![
                Shelf {
                    kind: Kind::Movie,
                    items: vec![],
                },
                Shelf {
                    kind: Kind::Show,
                    items: vec![],
                },
            ],
            headings: vec!["Trending".into(), "Popular TV".into()],
        };
        assert_eq!(p.filter(2).headings, ["Popular TV"]);
    }
}
