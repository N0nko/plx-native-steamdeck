//! File-version identity and selection, independent of UI and network state.
//!
//! Plex's mediaIndex is a position in the latest response, not a persistent ID.
//! Resolve it from the requested Part before each playback. Never substitute
//! version zero when an explicitly requested file disappeared or became ambiguous.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Position {
    pub(crate) media_index: i64,
    pub(crate) part_index: i64,
}

impl Position {
    pub(crate) const FIRST: Self = Self {
        media_index: 0,
        part_index: 0,
    };
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Candidate {
    pub(crate) position: Position,
    pub(crate) part_id: i64,
    pub(crate) part_key: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SelectionError {
    Missing,
    Ambiguous,
}

/// Parse only server-relative Plex Part URLs. A full URL, query-only value, zero,
/// negative or overflowing ID must never become a fuzzy match for another file.
pub(crate) fn part_id(key: &str) -> Option<i64> {
    let rest = key.strip_prefix("/library/parts/")?;
    let (id, tail) = rest.split_once('/')?;
    if id.is_empty() || tail.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    id.parse::<i64>().ok().filter(|id| *id > 0)
}

fn unique<I: Iterator<Item = usize>>(mut matches: I) -> Result<usize, SelectionError> {
    let first = matches.next().ok_or(SelectionError::Missing)?;
    if matches.next().is_some() {
        Err(SelectionError::Ambiguous)
    } else {
        Ok(first)
    }
}

/// Return an index into `candidates`, whose captured position addresses the fresh
/// Media[] response. A changed Part URL is accepted only for the same positive
/// Part ID. Identical candidates are rejected rather than guessed between.
pub(crate) fn choose(candidates: &[Candidate], requested: &str) -> Result<usize, SelectionError> {
    if requested.is_empty() {
        return candidates
            .iter()
            .position(|c| !c.part_key.is_empty())
            .ok_or(SelectionError::Missing);
    }
    let exact = unique(
        candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.part_key == requested)
            .map(|(i, _)| i),
    );
    if exact != Err(SelectionError::Missing) {
        return exact;
    }
    let id = part_id(requested).ok_or(SelectionError::Missing)?;
    unique(
        candidates
            .iter()
            .enumerate()
            .filter(|(_, c)| c.part_id == id && part_id(&c.part_key) == Some(id))
            .map(|(i, _)| i),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(media: i64, id: i64, stamp: &str) -> Candidate {
        Candidate {
            position: Position {
                media_index: media,
                part_index: 0,
            },
            part_id: id,
            part_key: format!("/library/parts/{id}/{stamp}/file.mkv"),
        }
    }

    #[test]
    fn selects_second_version_not_first() {
        let list = vec![candidate(0, 11, "a"), candidate(1, 22, "b")];
        let i = choose(&list, &list[1].part_key).unwrap();
        assert_eq!(list[i].position.media_index, 1);
        assert_eq!(list[i].part_id, 22);
    }

    #[test]
    fn rescans_rebind_the_position_not_the_file() {
        let requested = candidate(0, 22, "old");
        let list = vec![candidate(0, 11, "new"), candidate(1, 22, "new")];
        let i = choose(&list, &requested.part_key).unwrap();
        assert_eq!(list[i].position.media_index, 1);
        assert_eq!(list[i].part_key, "/library/parts/22/new/file.mkv");
    }

    #[test]
    fn missing_explicit_file_never_falls_back_to_zero() {
        assert_eq!(
            choose(&[candidate(0, 11, "a")], &candidate(1, 22, "a").part_key),
            Err(SelectionError::Missing)
        );
    }

    #[test]
    fn exact_key_wins_over_id_fallback() {
        let list = vec![candidate(0, 11, "a"), candidate(1, 11, "b")];
        assert_eq!(choose(&list, &list[1].part_key), Ok(1));
        assert_eq!(
            choose(&list, &candidate(9, 11, "c").part_key),
            Err(SelectionError::Ambiguous)
        );
    }

    #[test]
    fn duplicate_exact_keys_are_not_silently_selected() {
        let c = candidate(0, 11, "a");
        assert_eq!(
            choose(&[c.clone(), c.clone()], &c.part_key),
            Err(SelectionError::Ambiguous)
        );
    }

    #[test]
    fn implicit_selection_skips_empty_entries_without_renumbering() {
        let list = vec![Candidate::default(), candidate(4, 11, "a")];
        let i = choose(&list, "").unwrap();
        assert_eq!(list[i].position.media_index, 4);
    }

    #[test]
    fn malformed_ids_cannot_match() {
        for key in [
            "",
            "/library/parts/0/x",
            "/library/parts/-1/x",
            "/library/parts/1",
            "/library/parts/1/",
            "/library/parts/1e2/x",
            "https://x/library/parts/1/x",
            "/library/parts/999999999999999999999999/x",
            "/library/parts/1?/x",
        ] {
            assert_eq!(part_id(key), None, "{key}");
        }
        assert_eq!(part_id("/library/parts/42/a/file.mkv"), Some(42));
    }

    #[test]
    fn empty_library_fails_closed() {
        assert_eq!(choose(&[], ""), Err(SelectionError::Missing));
        assert_eq!(
            choose(&[], "/library/parts/1/a/file.mkv"),
            Err(SelectionError::Missing)
        );
    }

    #[test]
    fn first_position_is_explicit_and_defaulted() {
        assert_eq!(Position::FIRST, Position::default());
        assert_eq!(Position::FIRST.part_index, 0);
    }
    #[test]
    fn contradictory_part_id_and_url_cannot_be_a_fuzzy_match() {
        let mut c = candidate(0, 11, "new");
        c.part_id = 22;
        assert_eq!(
            choose(&[c], "/library/parts/22/old/file.mkv"),
            Err(SelectionError::Missing)
        );
    }
}
