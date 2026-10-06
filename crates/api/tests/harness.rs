//! AC8: the client talks to an injectable base URL; no real network.

use serde::Deserialize;
use tidal_player_api::Client;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[derive(Deserialize, Debug, PartialEq)]
struct Echo {
    message: String,
    count: u32,
}

#[tokio::test]
async fn ac8_get_json_from_fixture() {
    let fixture = include_str!("fixtures/echo.json");
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/echo"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(fixture, "application/json"))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::new(server.uri()).expect("client");
    let result = client.get_json::<Echo>("/v1/echo").await;

    let expected = Echo {
        message: "hello".into(),
        count: 2,
    };
    assert_eq!(result.ok(), Some(expected));
}
