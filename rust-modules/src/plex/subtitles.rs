//! Plex on-demand subtitle search. Provider URLs are passed back to PMS, never fetched by the TV.
use super::{client::QueryBuilder, Client};
use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct Candidate {
    pub key: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub codec: String,
    #[serde(default)]
    pub language: String,
    #[serde(rename = "languageCode", default)]
    pub language_code: String,
    #[serde(rename = "providerTitle", default)]
    pub provider: String,
}

impl Candidate {
    pub(crate) fn text(&self) -> bool {
        matches!(
            self.codec.to_ascii_lowercase().as_str(),
            "srt" | "ass" | "ssa" | "vtt" | "webvtt" | "subrip"
        )
    }
}

fn endpoint(rk: &str) -> Option<String> {
    (!rk.is_empty() && rk.bytes().all(|c| c.is_ascii_digit()))
        .then(|| format!("/library/metadata/{rk}/subtitles"))
}

fn parse(body: &[u8]) -> Option<Vec<Candidate>> {
    #[derive(Deserialize)]
    struct Envelope {
        #[serde(rename = "MediaContainer")]
        container: Container,
    }
    #[derive(Deserialize)]
    struct Container {
        #[serde(rename = "Stream", default)]
        streams: Vec<Candidate>,
    }
    let reply: Envelope = serde_json::from_slice(body).ok()?;
    Some(
        reply
            .container
            .streams
            .into_iter()
            .filter(|s| s.text() && s.key.starts_with("/library/streams/"))
            .take(100)
            .collect(),
    )
}

impl Client {
    pub(crate) fn search_subtitles(&self, rk: &str, language: &str) -> Option<Vec<Candidate>> {
        let path = QueryBuilder::new(endpoint(rk)?)
            .str("language", language)
            .int("hearingImpaired", 0)
            .int("forced", 0)
            .build();
        parse(&self.subtitle_json(&path)?)
    }

    pub(crate) fn download_subtitle(&self, rk: &str, media_id: i64, candidate: &Candidate) -> bool {
        let Some(path) = endpoint(rk) else {
            return false;
        };
        if !candidate.text() || !candidate.key.starts_with("/library/streams/") || media_id <= 0 {
            return false;
        }
        let path = QueryBuilder::new(path)
            .str("key", &candidate.key)
            .int("mediaItemID", media_id)
            .build();
        (200..300).contains(&self.put(&path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "devtriggers")]
    #[test]
    fn search_and_download_use_pms_and_encode_the_provider_key_as_one_parameter() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            for verb in ["GET", "PUT"] {
                let (mut socket, _) = crate::testnet::accept(&listener).unwrap();
                socket
                    .set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                let mut buf = [0; 8192];
                let n = socket.read(&mut buf).unwrap();
                let request = String::from_utf8_lossy(&buf[..n]);
                assert!(request.starts_with(&format!("{verb} /library/metadata/42/subtitles?")));
                let body = if verb == "GET" {
                    assert!(request.contains("language=lt"));
                    assert!(request.contains("hearingImpaired=0") && request.contains("forced=0"));
                    assert!(request.contains("Accept: application/json"));
                    r#"{"MediaContainer":{"Stream":[{"key":"/library/streams/9?provider=OpenSubtitles&url=https://provider/file","codec":"srt","title":"release"}]}}"#
                } else {
                    assert!(request.contains("mediaItemID=77"));
                    assert!(request.contains(
                        "key=%2Flibrary%2Fstreams%2F9%3Fprovider%3DOpenSubtitles%26url%3D"
                    ));
                    assert!(!request.contains("&url="));
                    ""
                };
                write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                )
                .unwrap();
            }
        });
        let client = Client::new(
            super::super::ServerId::UNSET,
            "fixture",
            super::super::Origin::http("127.0.0.1", port.into()),
            "",
            "cid",
        );
        let results = client.search_subtitles("42", "lt").unwrap();
        assert!(client.download_subtitle("42", 77, &results[0]));
        server.join().unwrap();
    }
    #[test]
    fn subtitle_results_keep_provider_order_and_only_offer_renderable_text() {
        let rows = parse(
            br#"{"MediaContainer":{"Stream":[
            {"key":"/library/streams/2?provider=OpenSubtitles","codec":"srt","title":"release B"},
            {"key":"/library/streams/1","codec":"SRT","title":"release A"},
            {"key":"/library/streams/3","codec":"pgs"},
            {"key":"https://elsewhere/subtitle","codec":"srt"}]}}"#,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].title, "release B");
        assert_eq!(rows[1].title, "release A");
        assert!(parse(b"{}").is_none());
        assert!(parse(br#"{"MediaContainer":{"size":0}}"#)
            .unwrap()
            .is_empty());
        assert!(endpoint("12/../13").is_none());
    }
}
