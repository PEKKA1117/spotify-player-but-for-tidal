//! Spec 0003 AC24: `HttpSource` against `wiremock`. Retry delays go through
//! an injected sleeper that never waits; reads wait at most the source
//! contract's 100 ms each, bounded by a deadline.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use tidal_player::http_source::{HttpSource, HttpSourceConfig, Reresolve};
use tidal_player_api::auth::ManualClock;
use tidal_player_api::stream::{Codec, ResolvedStream, Segment, StreamError, StreamPlan};
use tidal_player_audio::{ReadOutcome, SegmentSpan, SourceError, SourceLayout, TrackSource};
use tidal_player_core::AudioQuality;
use tokio::runtime::Runtime;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn rt() -> Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap()
}

/// `n` bytes of a recognisable pattern.
fn bytes(n: usize, seed: u8) -> Vec<u8> {
    (0..n)
        .map(|i| (i as u8).wrapping_mul(7).wrapping_add(seed))
        .collect()
}

fn single(url: String) -> ResolvedStream {
    ResolvedStream {
        track_id: 123,
        quality: AudioQuality::Lossless,
        bit_depth: Some(16),
        sample_rate: Some(44_100),
        plan: StreamPlan::Single {
            url,
            codec: Codec::Flac,
        },
    }
}

const TIMESCALE: u32 = 44_100;
const SEGMENT_TICKS: u64 = 176_128;

fn segmented(base: &str, count: u64) -> ResolvedStream {
    ResolvedStream {
        track_id: 123,
        quality: AudioQuality::Lossless,
        bit_depth: Some(16),
        sample_rate: Some(44_100),
        plan: StreamPlan::Segmented {
            init_url: format!("{base}/0.mp4"),
            segments: (1..=count)
                .map(|n| Segment {
                    number: n,
                    url: format!("{base}/{n}.mp4"),
                    start: (n - 1) * SEGMENT_TICKS,
                    duration: SEGMENT_TICKS,
                    timescale: TIMESCALE,
                })
                .collect(),
            codec: Codec::Flac,
        },
    }
}

fn config(read_ahead: usize, clock: &Arc<ManualClock>) -> HttpSourceConfig {
    HttpSourceConfig {
        read_ahead,
        retry_delays: vec![
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(2),
        ],
        network_timeout: Duration::from_secs(5),
        sleeper: clock.clone(),
    }
}

fn clock() -> Arc<ManualClock> {
    Arc::new(ManualClock::new(SystemTime::UNIX_EPOCH))
}

/// A re-resolve callback answering `stream`, and how often it was called.
fn reresolve_to(stream: Option<ResolvedStream>) -> (Reresolve, Arc<AtomicUsize>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let callback: Reresolve = Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        let answer = stream.clone().ok_or(StreamError::NotFound);
        Box::pin(async move { answer })
    });
    (callback, calls)
}

/// Reads until the end or an error, in reads of at most `chunk` bytes.
fn read_all(source: &mut HttpSource, chunk: usize) -> (Vec<u8>, Option<SourceError>) {
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut out = Vec::new();
    let mut buf = vec![0; chunk];
    while Instant::now() < deadline {
        match source.read(&mut buf) {
            Ok(ReadOutcome::Data(n)) => out.extend_from_slice(&buf[..n]),
            Ok(ReadOutcome::Pending) => {}
            Ok(ReadOutcome::End) => return (out, None),
            Err(e) => return (out, Some(e)),
        }
    }
    panic!("the source neither ended nor failed within 20 s");
}

/// `(path, Range header)` of every request the server saw, in order.
fn requests(rt: &Runtime, server: &MockServer) -> Vec<(String, Option<String>)> {
    rt.block_on(server.received_requests())
        .unwrap_or_default()
        .into_iter()
        .map(|r| {
            let range = r
                .headers
                .get("range")
                .map(|v| v.to_str().unwrap().to_owned());
            (r.url.path().to_owned(), range)
        })
        .collect()
}

fn partial(body: &[u8], from: usize, to: usize) -> ResponseTemplate {
    ResponseTemplate::new(206)
        .insert_header(
            "content-range",
            format!("bytes {from}-{}/{}", body.len() - 1, body.len()).as_str(),
        )
        .set_body_bytes(body[from..to].to_vec())
}

fn mount_range(
    rt: &Runtime,
    server: &MockServer,
    p: &str,
    range: &str,
    response: ResponseTemplate,
) {
    rt.block_on(
        Mock::given(method("GET"))
            .and(path(p))
            .and(header("range", range))
            .respond_with(response)
            .mount(server),
    );
}

#[test]
fn ac24_range_resume() {
    let rt = rt();
    let server = rt.block_on(MockServer::start());
    let body = bytes(1000, 3);
    // The first response stops after 400 of the 1000 bytes it announced.
    mount_range(&rt, &server, "/a.flac", "bytes=0-", partial(&body, 0, 400));
    mount_range(
        &rt,
        &server,
        "/a.flac",
        "bytes=400-",
        partial(&body, 400, 1000),
    );
    mount_range(
        &rt,
        &server,
        "/a.flac",
        "bytes=900-",
        partial(&body, 900, 1000),
    );
    let clock = clock();
    let (reresolve, calls) = reresolve_to(None);
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            single(format!("{}/a.flac", server.uri())),
            reresolve,
            config(64 * 1024, &clock),
        ))
        .expect("open");

    assert_eq!(
        source.layout(),
        SourceLayout::SingleFile { len: Some(1000) }
    );
    assert_eq!(source.extension_hint().as_deref(), Some("flac"));
    let (data, error) = read_all(&mut source, 256);
    assert_eq!(error, None);
    assert_eq!(data, body, "the resumed read continues at the exact offset");
    assert_eq!(clock.sleeps(), vec![Duration::from_millis(500)]);

    // Seeking re-requests from the seek point only.
    source.seek(900).unwrap();
    let (data, error) = read_all(&mut source, 256);
    assert_eq!(error, None);
    assert_eq!(data, body[900..]);
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let ranges: Vec<_> = requests(&rt, &server)
        .into_iter()
        .map(|(_, range)| range)
        .collect();
    assert_eq!(
        ranges,
        [Some("bytes=0-"), Some("bytes=400-"), Some("bytes=900-")]
            .map(|r| r.map(String::from))
            .to_vec()
    );
}

#[test]
fn ac24_retry_then_fail() {
    let rt = rt();
    let server = rt.block_on(MockServer::start());
    let body = bytes(1000, 5);
    mount_range(&rt, &server, "/a.flac", "bytes=0-", partial(&body, 0, 400));
    mount_range(
        &rt,
        &server,
        "/a.flac",
        "bytes=400-",
        ResponseTemplate::new(503),
    );
    let clock = clock();
    let (reresolve, _) = reresolve_to(None);
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            single(format!("{}/a.flac", server.uri())),
            reresolve,
            config(64 * 1024, &clock),
        ))
        .expect("open");

    let (data, error) = read_all(&mut source, 256);
    assert_eq!(data, body[..400], "every byte before the cut is delivered");
    assert!(
        matches!(error, Some(SourceError::Network(_))),
        "fails with Network after the retries: {error:?}"
    );
    assert_eq!(
        clock.sleeps(),
        vec![
            Duration::from_millis(500),
            Duration::from_secs(1),
            Duration::from_secs(2)
        ]
    );
    let resumes = requests(&rt, &server)
        .into_iter()
        .filter(|(_, range)| range.as_deref() == Some("bytes=400-"))
        .count();
    assert_eq!(resumes, 3, "three retries, each from the exact offset");
}

#[test]
fn ac24_reresolve_on_403() {
    // Single file: the old URL expires mid-stream; the new one continues
    // from the same offset.
    let rt = rt();
    let server = rt.block_on(MockServer::start());
    let body = bytes(1000, 7);
    mount_range(
        &rt,
        &server,
        "/old.flac",
        "bytes=0-",
        partial(&body, 0, 400),
    );
    mount_range(
        &rt,
        &server,
        "/old.flac",
        "bytes=400-",
        ResponseTemplate::new(403),
    );
    mount_range(
        &rt,
        &server,
        "/new.flac",
        "bytes=400-",
        partial(&body, 400, 1000),
    );
    let clock = clock();
    let (reresolve, calls) = reresolve_to(Some(single(format!("{}/new.flac", server.uri()))));
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            single(format!("{}/old.flac", server.uri())),
            reresolve,
            config(64 * 1024, &clock),
        ))
        .expect("open");
    let (data, error) = read_all(&mut source, 256);
    assert_eq!(error, None);
    assert_eq!(data, body);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    // Segmented: segment 2 expires; the new plan continues at segment 2.
    let server = rt.block_on(MockServer::start());
    let (init, s1, s2) = (bytes(50, 1), bytes(300, 2), bytes(300, 3));
    let ok = |b: &[u8]| ResponseTemplate::new(200).set_body_bytes(b.to_vec());
    let mount = |p: &str, r: ResponseTemplate| {
        rt.block_on(
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(r)
                .mount(&server),
        );
    };
    mount("/old/0.mp4", ok(&init));
    mount("/old/1.mp4", ok(&s1));
    mount("/old/2.mp4", ResponseTemplate::new(410));
    mount("/new/2.mp4", ok(&s2));
    let base = server.uri();
    let (reresolve, calls) = reresolve_to(Some(segmented(&format!("{base}/new"), 2)));
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            segmented(&format!("{base}/old"), 2),
            reresolve,
            config(64 * 1024, &clock),
        ))
        .expect("open");
    let (data, error) = read_all(&mut source, 256);
    assert_eq!(error, None);
    assert_eq!(data, [init.clone(), s1.clone(), s2.clone()].concat());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let paths: Vec<_> = requests(&rt, &server).into_iter().map(|(p, _)| p).collect();
    assert_eq!(
        paths,
        ["/old/0.mp4", "/old/1.mp4", "/old/2.mp4", "/new/2.mp4"]
    );

    // A second 403, on the new URL, fails the track.
    let server = rt.block_on(MockServer::start());
    mount_range(
        &rt,
        &server,
        "/old.flac",
        "bytes=0-",
        partial(&body, 0, 400),
    );
    mount_range(
        &rt,
        &server,
        "/old.flac",
        "bytes=400-",
        ResponseTemplate::new(403),
    );
    mount_range(
        &rt,
        &server,
        "/new.flac",
        "bytes=400-",
        ResponseTemplate::new(403),
    );
    let (reresolve, calls) = reresolve_to(Some(single(format!("{}/new.flac", server.uri()))));
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            single(format!("{}/old.flac", server.uri())),
            reresolve,
            config(64 * 1024, &clock),
        ))
        .expect("open");
    let (data, error) = read_all(&mut source, 256);
    assert_eq!(data, body[..400]);
    assert!(error.is_some(), "a second 403 fails");
    assert_eq!(calls.load(Ordering::SeqCst), 1, "re-resolved only once");
}

#[test]
fn ac24_segmented_order() {
    let rt = rt();
    let server = rt.block_on(MockServer::start());
    let init = bytes(60, 9);
    let segments: Vec<Vec<u8>> = (0..3).map(|n| bytes(500 + n * 10, n as u8)).collect();
    let mount = |p: &str, b: &[u8]| {
        rt.block_on(
            Mock::given(method("GET"))
                .and(path(p))
                .respond_with(ResponseTemplate::new(200).set_body_bytes(b.to_vec()))
                .mount(&server),
        );
    };
    mount("/s/0.mp4", &init);
    for (n, segment) in segments.iter().enumerate() {
        mount(&format!("/s/{}.mp4", n + 1), segment);
    }
    let clock = clock();
    let (reresolve, _) = reresolve_to(None);
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            segmented(&format!("{}/s", server.uri()), 3),
            reresolve,
            config(64 * 1024, &clock),
        ))
        .expect("open");

    assert_eq!(
        source.layout(),
        SourceLayout::Segmented {
            timescale: TIMESCALE,
            segments: (0..3)
                .map(|n| SegmentSpan {
                    start: n * SEGMENT_TICKS,
                    duration: SEGMENT_TICKS
                })
                .collect(),
        }
    );
    assert_eq!(source.extension_hint().as_deref(), Some("mp4"));
    let (data, error) = read_all(&mut source, 100);
    assert_eq!(error, None);
    assert_eq!(
        data,
        [vec![init.clone()], segments.clone()].concat().concat()
    );

    // A restart at segment 2 (0-based) reads the init segment, then
    // segment 2 only: nothing before it is fetched again.
    source.restart_at_segment(2).unwrap();
    let (data, error) = read_all(&mut source, 100);
    assert_eq!(error, None);
    assert_eq!(data, [init.clone(), segments[2].clone()].concat());

    let seen = requests(&rt, &server);
    let paths: Vec<_> = seen.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "/s/0.mp4", "/s/1.mp4", "/s/2.mp4", "/s/3.mp4", "/s/0.mp4", "/s/3.mp4"
        ]
    );
    assert!(
        seen.iter().all(|(_, range)| range.is_none()),
        "whole segments are fetched without Range: {seen:?}"
    );
}

#[test]
fn ac24_readahead_cap() {
    let rt = rt();
    let server = rt.block_on(MockServer::start());
    let body = bytes(64 * 1024, 11);
    mount_range(
        &rt,
        &server,
        "/big.flac",
        "bytes=0-",
        partial(&body, 0, body.len()),
    );
    let cap = 4096;
    let clock = clock();
    let (reresolve, _) = reresolve_to(None);
    let mut source = rt
        .block_on(HttpSource::open(
            reqwest::Client::new(),
            single(format!("{}/big.flac", server.uri())),
            reresolve,
            config(cap, &clock),
        ))
        .expect("open");

    // The fetcher fills the buffer to the cap, then waits for the reader.
    assert!(source.wait_buffered(cap, Duration::from_secs(10)));
    assert_eq!(source.buffered(), cap);
    let (data, error) = read_all(&mut source, 1000);
    assert_eq!(error, None);
    assert_eq!(data, body);
    assert_eq!(source.high_water(), cap, "never more than the cap ahead");
}
