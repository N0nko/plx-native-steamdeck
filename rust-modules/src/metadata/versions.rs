//! Version snapshots and main-thread selection. The player re-resolves the Part
//! against fresh metadata; this snapshot is for the UI and companion controls.
use super::{convert_streams, Detail, Dovi, Stream};
use crate::media_version::{Candidate, Position};

#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub(crate) struct MediaVersion {
    pub(crate) source: Candidate,
    pub(crate) part_count: usize,
    pub(crate) preview_part_id: Option<i64>,
    pub(crate) vcodec: String,
    pub(crate) acodec: String,
    pub(crate) video_resolution: String,
    pub(crate) width: i64,
    pub(crate) height: i64,
    pub(crate) bitrate: i64,
    pub(crate) container: String,
    pub(crate) file: String,
    pub(crate) size: i64,
    #[serde(with = "super::record::float_bits")]
    pub(crate) aspect_ratio: f64,
    #[serde(with = "super::record::float_bits")]
    pub(crate) video_fps: f64,
    pub(crate) video: Option<Stream>,
    pub(crate) audio: Vec<Stream>,
    pub(crate) subs: Vec<Stream>,
    pub(crate) hdr: bool,
    pub(crate) dovi: Dovi,
}

impl MediaVersion {
    pub(crate) fn playable(&self) -> bool {
        self.part_count == 1 && !self.source.part_key.is_empty()
    }

    pub(crate) fn source_kbps(&self) -> i64 {
        self.video
            .as_ref()
            .map(|s| s.bitrate)
            .filter(|b| *b > 0)
            .unwrap_or(self.bitrate)
    }

    fn apply_detail(&self, d: &mut Detail) {
        d.preview_part_id = self.preview_part_id;
        d.part = self.source.part_key.clone();
        d.vcodec = self.vcodec.clone();
        d.acodec = self.acodec.clone();
        d.video_resolution = self.video_resolution.clone();
        d.width = self.width;
        d.height = self.height;
        d.bitrate = self.bitrate;
        d.container = self.container.clone();
        d.file = self.file.clone();
        d.size = self.size;
        d.aspect_ratio = self.aspect_ratio;
        d.video_fps = self.video_fps;
        d.video = self.video.clone();
        d.audio = self.audio.clone();
        d.subs = self.subs.clone();
        d.hdr = self.hdr;
        d.dovi = self.dovi;
    }

    fn apply_episode(&self, ep: &mut super::Episode) {
        ep.part = self.source.part_key.clone();
        ep.vcodec = self.vcodec.clone();
        ep.acodec = self.acodec.clone();
    }
}

pub(crate) fn collect_versions(it: &crate::plex::Metadata) -> Vec<MediaVersion> {
    it.media
        .iter()
        .enumerate()
        .filter_map(|(i, media)| {
            let part = media.part.first()?;
            let streams = convert_streams(&part.stream);
            Some(MediaVersion {
                source: Candidate {
                    position: Position {
                        media_index: i64::try_from(i).ok()?,
                        part_index: 0,
                    },
                    part_id: if part.id > 0 {
                        part.id
                    } else {
                        crate::media_version::part_id(&part.key).unwrap_or(0)
                    },
                    part_key: part.key.clone(),
                },
                part_count: media.part.len(),
                preview_part_id: part.preview_part_id(),
                vcodec: media.video_codec.clone(),
                acodec: media.audio_codec.clone(),
                video_resolution: media.video_resolution.clone(),
                width: media.width,
                height: media.height,
                bitrate: media.bitrate,
                container: if part.container.is_empty() {
                    media.container.clone()
                } else {
                    part.container.clone()
                },
                file: part.file.clone(),
                size: part.size,
                aspect_ratio: media.aspect_ratio,
                video_fps: streams.fps,
                video: streams.video,
                audio: streams.audio,
                subs: streams.subs,
                hdr: streams.hdr,
                dovi: streams.dovi,
            })
        })
        .collect()
}

/// The menu's captured server and item must still be mounted. Update the
/// selected Part as well as its technical fields so Repair/Get Ready cannot
/// accidentally act on version zero after a selection.
pub(crate) fn select_version(
    state: &mut super::MetadataState,
    sid: crate::plex::ServerId,
    rk: &str,
    requested: &str,
) -> bool {
    let Some(d) = state.current.as_mut() else {
        return false;
    };
    if d.sid != sid {
        return false;
    }
    let list = if d.rk == rk {
        &d.versions
    } else if let Some(ep) = d.on_deck.as_ref().filter(|ep| ep.rk == rk) {
        &ep.versions
    } else if let Some(ep) = d.episodes.iter().find(|ep| ep.rk == rk) {
        &ep.versions
    } else {
        return false;
    };
    let candidates: Vec<_> = list.iter().map(|v| v.source.clone()).collect();
    let Ok(i) = crate::media_version::choose(&candidates, requested) else {
        return false;
    };
    let v = list[i].clone();
    if !v.playable() {
        return false;
    }
    if d.rk == rk {
        v.apply_detail(d);
    } else {
        if let Some(ep) = d.on_deck.as_mut().filter(|ep| ep.rk == rk) {
            v.apply_episode(ep);
        }
        for ep in d.episodes.iter_mut().filter(|ep| ep.rk == rk) {
            v.apply_episode(ep);
        }
    }
    true
}

/// The leaf the show hero would play, using on-deck only once the show is underway.
/// Shared by the Detail owner and version surface so neither screen depends on its sibling.
pub(crate) fn version_episode(d: &Detail) -> Option<&super::Episode> {
    let started = d.on_deck.as_ref().is_some_and(|ep| ep.resume_ms > 0)
        || d.seasons.iter().any(|season| season.viewed_leaf_count > 0);
    let ep = d
        .on_deck
        .as_ref()
        .filter(|_| started)
        .or_else(|| d.episodes.first())?;
    d.episodes
        .iter()
        .find(|other| other.rk == ep.rk)
        .or(Some(ep))
}

pub(crate) fn version_target(d: &Detail) -> Option<(&str, &str, &[MediaVersion])> {
    if d.is_show {
        let ep = version_episode(d)?;
        Some((&ep.rk, &ep.part, &ep.versions))
    } else {
        Some((&d.rk, &d.part, &d.versions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item() -> crate::plex::Metadata {
        serde_json::from_str(
            r#"{
          "ratingKey":"42", "type":"movie", "Media":[
            {"videoCodec":"hevc","audioCodec":"truehd","videoResolution":"4k",
             "width":3840,"height":2160,"bitrate":60000,
             "Part":[{"id":11,"key":"/library/parts/11/a/file.mkv","size":900000,
               "Stream":[{"id":101,"streamType":2,"codec":"truehd","index":1}]}]},
            {"videoCodec":"h264","audioCodec":"aac","videoResolution":"1080",
             "width":1920,"height":1080,"bitrate":8000,
             "Part":[{"id":22,"key":"/library/parts/22/a/file.mp4","size":80000,
               "Stream":[{"id":202,"streamType":2,"codec":"aac","index":1}]}]}
          ]}"#,
        )
        .unwrap()
    }

    #[test]
    fn each_version_keeps_its_own_streams_and_bitrate() {
        let v = collect_versions(&item());
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].source.position.media_index, 0);
        assert_eq!(v[1].source.position.media_index, 1);
        assert_eq!(v[1].audio[0].id, 202);
        assert_eq!(v[1].width, 1920);
        assert_eq!(v[1].bitrate, 8000);
        assert_eq!(v[1].vcodec, "h264");
    }

    #[test]
    fn apply_updates_companion_part_and_technical_record_together() {
        let v = collect_versions(&item());
        let mut d = Detail::default();
        v[1].apply_detail(&mut d);
        assert_eq!(d.part, v[1].source.part_key);
        assert_eq!(d.audio[0].id, 202);
        assert_eq!(d.width, 1920);
        assert_eq!(d.size, 80000);
    }

    #[test]
    fn playing_projection_uses_selected_version_not_media_zero() {
        let p = super::super::playing_from_metadata(
            crate::plex::ServerId::UNSET,
            "42",
            "/library/parts/22/a/file.mp4",
            &item(),
        )
        .unwrap();
        assert_eq!(p.audio[0].id, 202);
        assert_eq!(p.width, 1920);
        assert_eq!(p.version.unwrap().source.position.media_index, 1);
    }

    #[test]
    fn missing_selected_version_has_no_playable_projection() {
        assert!(super::super::playing_from_metadata(
            crate::plex::ServerId::UNSET,
            "42",
            "/library/parts/999/a/file.mkv",
            &item()
        )
        .is_none());
    }

    #[test]
    fn multipart_versions_are_not_presented_as_complete_playback() {
        let mut it = item();
        it.media[1].part.push(crate::plex::MediaPart::default());
        let v = collect_versions(&it);
        assert!(!v[1].playable());
        assert!(super::super::playing_from_metadata(
            crate::plex::ServerId::UNSET,
            "42",
            "/library/parts/22/a/file.mp4",
            &it
        )
        .is_none());
    }
    #[test]
    fn selection_cannot_cross_servers_or_mutate_an_unrelated_item() {
        let _guard = crate::testlock::serial();
        let mut state = super::super::MetadataState::default();
        let sid = crate::plex::ServerId::from_raw(1);
        let other = crate::plex::ServerId::from_raw(2);
        let v = collect_versions(&item());
        let mut d = Detail {
            sid,
            rk: "42".into(),
            versions: v.clone(),
            ..Default::default()
        };
        v[0].apply_detail(&mut d);
        super::super::set_current_for_test(&mut state, Some(d));
        assert!(!select_version(
            &mut state,
            other,
            "42",
            &v[1].source.part_key
        ));
        assert!(!select_version(
            &mut state,
            sid,
            "99",
            &v[1].source.part_key
        ));
        assert_eq!(
            super::super::current(&state).unwrap().part,
            v[0].source.part_key
        );
        assert!(select_version(&mut state, sid, "42", &v[1].source.part_key));
        assert_eq!(
            super::super::current(&state).unwrap().part,
            v[1].source.part_key
        );
        assert_eq!(super::super::current(&state).unwrap().audio[0].id, 202);
        super::super::set_current_for_test(&mut state, None);
    }

    #[test]
    fn episode_selection_updates_on_deck_and_the_season_row_together() {
        let _guard = crate::testlock::serial();
        let mut state = super::super::MetadataState::default();
        let sid = crate::plex::ServerId::from_raw(1);
        let v = collect_versions(&item());
        let ep = super::super::Episode {
            rk: "42".into(),
            part: v[0].source.part_key.clone(),
            versions: v.clone(),
            ..Default::default()
        };
        super::super::set_current_for_test(
            &mut state,
            Some(Detail {
                sid,
                rk: "show".into(),
                is_show: true,
                on_deck: Some(ep.clone()),
                episodes: vec![ep],
                ..Default::default()
            }),
        );
        assert!(select_version(&mut state, sid, "42", &v[1].source.part_key));
        let d = super::super::current(&state).unwrap();
        assert_eq!(d.on_deck.as_ref().unwrap().part, v[1].source.part_key);
        assert_eq!(d.episodes[0].part, v[1].source.part_key);
        assert_eq!(d.episodes[0].vcodec, "h264");
        super::super::set_current_for_test(&mut state, None);
    }

    #[test]
    fn a_rescan_reorder_rebinds_the_file_to_its_new_wire_index() {
        let mut it = item();
        it.media.swap(0, 1);
        let p = super::super::playing_from_metadata(
            crate::plex::ServerId::UNSET,
            "42",
            "/library/parts/22/a/file.mp4",
            &it,
        )
        .unwrap();
        assert_eq!(p.audio[0].id, 202);
        assert_eq!(p.version.unwrap().source.position.media_index, 0);
    }
    #[test]
    fn a_metadata_response_for_another_item_cannot_play() {
        assert!(super::super::playing_from_metadata(
            crate::plex::ServerId::UNSET,
            "43",
            "/library/parts/22/a/file.mp4",
            &item()
        )
        .is_none());
    }
    #[test]
    fn nonfinite_technical_floats_survive_the_same_bit_preserving_recording_codec() {
        let v = MediaVersion {
            aspect_ratio: f64::NAN,
            video_fps: f64::INFINITY,
            ..Default::default()
        };
        let bytes = serde_json::to_vec(&v).unwrap();
        let restored: MediaVersion = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.aspect_ratio.to_bits(), v.aspect_ratio.to_bits());
        assert_eq!(restored.video_fps.to_bits(), v.video_fps.to_bits());
    }
}
