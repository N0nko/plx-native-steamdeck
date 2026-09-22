//! Pre-play track choices are scoped to one server, item and Part, not a global language.
//! None means use the server/show preference; Some(0) means subtitles explicitly Off.
use super::{MediaVersion, MetadataState, Stream};
use crate::plex::ServerId;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TrackChoice {
    pub(crate) audio: Option<i64>,
    pub(crate) subtitle: Option<i64>,
}

impl TrackChoice {
    pub(crate) fn valid(self, audio: &[Stream], subs: &[Stream]) -> bool {
        self.audio
            .is_none_or(|id| id > 0 && audio.iter().filter(|s| s.id == id).count() == 1)
            && self.subtitle.is_none_or(|id| {
                id == 0 || id > 0 && subs.iter().filter(|s| s.id == id).count() == 1
            })
    }
}

pub(super) struct Entry {
    sid: ServerId,
    rk: String,
    part: String,
    choice: TrackChoice,
}

pub(super) fn get(state: &MetadataState, sid: ServerId, rk: &str, part: &str) -> TrackChoice {
    state
        .preplay
        .iter()
        .find(|e| e.sid == sid && e.rk == rk && e.part == part)
        .map(|e| e.choice)
        .unwrap_or_default()
}

pub(super) fn set(
    state: &mut MetadataState,
    sid: ServerId,
    rk: &str,
    part: &str,
    choice: TrackChoice,
) -> bool {
    let Some(d) = state.current.as_ref().filter(|d| d.sid == sid) else {
        return false;
    };
    let versions: &[MediaVersion] = if d.rk == rk {
        &d.versions
    } else if let Some(ep) = d.on_deck.as_ref().filter(|ep| ep.rk == rk) {
        &ep.versions
    } else if let Some(ep) = d.episodes.iter().find(|ep| ep.rk == rk) {
        &ep.versions
    } else {
        return false;
    };
    let mut candidates = versions
        .iter()
        .filter(|v| v.source.part_key == part && v.playable());
    let Some(v) = candidates.next() else {
        return false;
    };
    if candidates.next().is_some() || !choice.valid(&v.audio, &v.subs) {
        return false;
    }
    if let Some(e) = state
        .preplay
        .iter_mut()
        .find(|e| e.sid == sid && e.rk == rk && e.part == part)
    {
        if e.choice == choice {
            return false;
        }
        e.choice = choice;
    } else {
        // A bounded page/session cache. Old choices cannot grow without limit across episodes.
        if state.preplay.len() == 16 {
            state.preplay.remove(0);
        }
        state.preplay.push(Entry {
            sid,
            rk: rk.into(),
            part: part.into(),
            choice,
        });
    }
    true
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ChoiceRow {
    pub(crate) id: Option<i64>,
    pub(crate) label: String,
    pub(crate) detail: String,
}

pub(crate) fn options(tracks: &[Stream], subtitles: bool) -> Vec<ChoiceRow> {
    if !subtitles && tracks.is_empty() {
        return Vec::new();
    }
    let mut rows = vec![ChoiceRow {
        id: None,
        label: "Automatic".into(),
        detail: "Use this item's server and show preferences".into(),
    }];
    if subtitles {
        rows.push(ChoiceRow {
            id: Some(0),
            label: "Off".into(),
            detail: "Do not display or burn subtitles".into(),
        });
    }
    for s in tracks.iter().filter(|s| s.id > 0) {
        // Malformed duplicate IDs cannot be safely selected, even when their labels differ.
        if tracks.iter().filter(|other| other.id == s.id).count() != 1 {
            continue;
        }
        let language = if s.lang.is_empty() {
            "Unknown language"
        } else {
            &s.lang
        };
        let mut details = vec![super::friendly_codec(&s.codec)];
        if s.channels > 0 {
            details.push(format!("{} channels", s.channels));
        }
        if s.forced {
            details.push("Forced".into());
        }
        if s.sdh {
            details.push("SDH".into());
        }
        if s.ad {
            details.push("Audio description".into());
        }
        if s.external {
            details.push("External".into());
        }
        if !s.title.is_empty() {
            details.push(s.title.clone());
        }
        rows.push(ChoiceRow {
            id: Some(s.id),
            label: language.into(),
            detail: details.join(" · "),
        });
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    fn track(id: i64) -> Stream {
        Stream {
            id,
            codec: "ac3".into(),
            ..Default::default()
        }
    }
    #[test]
    fn automatic_and_explicit_off_are_different() {
        assert_ne!(
            TrackChoice::default(),
            TrackChoice {
                subtitle: Some(0),
                ..Default::default()
            }
        );
        assert!(TrackChoice {
            subtitle: Some(0),
            ..Default::default()
        }
        .valid(&[], &[]));
    }
    #[test]
    fn rejects_missing_duplicate_and_negative_track_ids() {
        assert!(!TrackChoice {
            audio: Some(9),
            subtitle: None
        }
        .valid(&[track(1)], &[]));
        assert!(!TrackChoice {
            audio: Some(1),
            subtitle: None
        }
        .valid(&[track(1), track(1)], &[]));
        assert!(!TrackChoice {
            audio: None,
            subtitle: Some(-1)
        }
        .valid(&[], &[]));
        assert!(TrackChoice {
            audio: Some(1),
            subtitle: None
        }
        .valid(&[track(1)], &[]));
    }
    #[test]
    fn subtitle_options_preserve_off_and_automatic_even_when_no_tracks_exist() {
        let rows = options(&[], true);
        assert_eq!(
            rows.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![None, Some(0)]
        );
    }
    #[test]
    fn duplicate_ids_never_become_selectable_rows() {
        assert_eq!(options(&[track(1), track(1)], false).len(), 1);
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;
    fn version(id: i64) -> MediaVersion {
        MediaVersion {
            source: crate::media_version::Candidate {
                position: crate::media_version::Position::FIRST,
                part_id: id,
                part_key: format!("/library/parts/{id}/file.mkv"),
            },
            part_count: 1,
            audio: vec![Stream {
                id: id * 10,
                codec: "ac3".into(),
                ..Default::default()
            }],
            subs: vec![Stream {
                id: id * 10 + 1,
                codec: "srt".into(),
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    fn state() -> MetadataState {
        MetadataState {
            current: Some(super::super::Detail {
                sid: ServerId::from_raw(1),
                rk: "42".into(),
                versions: vec![version(1), version(2)],
                ..Default::default()
            }),
            ..Default::default()
        }
    }
    #[test]
    fn choices_are_file_server_and_item_scoped() {
        let mut s = state();
        let sid = ServerId::from_raw(1);
        let chosen = TrackChoice {
            audio: Some(10),
            subtitle: Some(0),
        };
        assert!(set(&mut s, sid, "42", &version(1).source.part_key, chosen));
        assert_eq!(get(&s, sid, "42", &version(1).source.part_key), chosen);
        for (server, rk, part) in [
            (sid, "42", version(2).source.part_key),
            (ServerId::from_raw(2), "42", version(1).source.part_key),
            (sid, "43", version(1).source.part_key),
        ] {
            assert_eq!(get(&s, server, rk, &part), TrackChoice::default());
        }
    }
    #[test]
    fn one_versions_stream_id_cannot_be_assigned_to_another_version() {
        let mut s = state();
        let sid = ServerId::from_raw(1);
        assert!(!set(
            &mut s,
            sid,
            "42",
            &version(2).source.part_key,
            TrackChoice {
                audio: Some(10),
                subtitle: None
            }
        ));
        assert!(s.preplay.is_empty());
    }
    #[test]
    fn duplicate_parts_and_wrong_items_cannot_mutate_choices() {
        let mut s = state();
        let sid = ServerId::from_raw(1);
        s.current.as_mut().unwrap().versions.push(version(1));
        assert!(!set(
            &mut s,
            sid,
            "42",
            &version(1).source.part_key,
            TrackChoice::default()
        ));
        assert!(!set(
            &mut s,
            sid,
            "wrong",
            &version(2).source.part_key,
            TrackChoice::default()
        ));
        assert!(s.preplay.is_empty());
    }
}
