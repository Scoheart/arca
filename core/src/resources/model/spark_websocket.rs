//! Signed WebSocket requests for the classic iFLYTEK Spark service.

use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use base64::prelude::*;
use ring::hmac;
use serde::Deserialize;
use serde_json::json;
use tungstenite::protocol::WebSocketConfig;
use tungstenite::{client_tls_with_config, Message};
use url::Url;
use zeroize::Zeroizing;

const HOST: &str = "spark-api.xf-yun.com";
const RESPONSE_LIMIT: usize = 1024 * 1024;

#[derive(Clone, Copy)]
pub enum SparkWebSocketModel {
    Ultra,
    Pro,
    Pro128k,
    Lite,
}

impl SparkWebSocketModel {
    fn address(self) -> (&'static str, &'static str) {
        match self {
            Self::Ultra => ("/v4.0/chat", "4.0Ultra"),
            Self::Pro => ("/v3.1/chat", "generalv3"),
            Self::Pro128k => ("/chat/pro-128k", "pro-128k"),
            Self::Lite => ("/v1.1/chat", "lite"),
        }
    }
}

#[derive(Deserialize)]
struct Frame {
    header: Header,
    payload: Option<Payload>,
}

#[derive(Deserialize)]
struct Header {
    code: i32,
    status: Option<u8>,
}

#[derive(Deserialize)]
struct Payload {
    choices: Option<Choices>,
}

#[derive(Deserialize)]
struct Choices {
    status: Option<u8>,
    text: Vec<Content>,
}

#[derive(Deserialize)]
struct Content {
    content: String,
}

pub fn request(model: SparkWebSocketModel, prompt: &str) -> Result<String, String> {
    if prompt.trim().is_empty() {
        return Err("spark_websocket_invalid: Enter a non-empty message".into());
    }
    let app_id = credential_from_env("IFLYTEK_SPARK_APP_ID")?;
    let api_key = credential_from_env("IFLYTEK_SPARK_API_KEY")?;
    let api_secret = credential_from_env("IFLYTEK_SPARK_API_SECRET")?;
    let (path, domain) = model.address();
    let date = chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();
    let url = signed_url(path, &date, api_key.as_str(), api_secret.as_bytes())?;
    let connect_deadline = Instant::now() + Duration::from_secs(10);
    let mut addresses = (HOST, 443).to_socket_addrs()
        .map_err(|_| "spark_websocket_network: Could not resolve the Spark hostname")?;
    let stream = addresses.find_map(|address| {
        let remaining = connect_deadline.saturating_duration_since(Instant::now());
        (!remaining.is_zero()).then(|| TcpStream::connect_timeout(&address, remaining).ok()).flatten()
    }).ok_or("spark_websocket_network: Could not connect to the Spark service")?;
    let socket_timeout = Some(Duration::from_secs(25));
    stream.set_read_timeout(socket_timeout).and_then(|_| stream.set_write_timeout(socket_timeout))
        .map_err(|_| "spark_websocket_network: Could not set Spark connection timeouts")?;
    let config = WebSocketConfig::default()
        .max_message_size(Some(RESPONSE_LIMIT))
        .max_frame_size(Some(RESPONSE_LIMIT));
    // This explicit TLS handshake cannot follow a redirect containing the signature.
    let (mut socket, _) = client_tls_with_config(url.as_str(), stream, Some(config), None)
        .map_err(|_| "spark_websocket_handshake: Spark refused the signed TLS connection. Check the service credentials and system clock")?;
    let message = json!({
        "header": {"app_id": app_id.as_str()},
        "parameter": {"chat": {"domain": domain}},
        "payload": {"message": {"text": [{"role": "user", "content": prompt}]}}
    });
    socket.send(Message::Text(message.to_string().into()))
        .map_err(|_| "spark_websocket_network: Could not send the Spark request")?;

    let deadline = Instant::now() + Duration::from_secs(120);
    let mut answer = String::new();
    while Instant::now() < deadline {
        match socket.read().map_err(|_| "spark_websocket_network: Spark ended the response before completion")? {
            Message::Text(message) => {
                let frame: Frame = serde_json::from_str(&message)
                    .map_err(|_| "spark_websocket_response: Spark returned an invalid response frame")?;
                if append_frame(frame, &mut answer)? {
                    let _ = socket.close(None);
                    return Ok(answer);
                }
            }
            Message::Close(_) => return Err("spark_websocket_response: Spark closed before the final frame".into()),
            _ => {}
        }
    }
    Err("spark_websocket_timeout: Spark did not finish within the request time limit".into())
}

fn credential_from_env(name: &str) -> Result<Zeroizing<String>, String> {
    std::env::var(name).ok().filter(|value| !value.trim().is_empty() && !value.contains(['\r', '\n']))
        .map(Zeroizing::new)
        .ok_or_else(|| format!("spark_websocket_credential: Set {name} for the Spark WebSocket service"))
}

fn signed_url(path: &str, date: &str, api_key: &str, api_secret: &[u8]) -> Result<Url, String> {
    if api_key.contains(['"', '\\']) {
        return Err("spark_websocket_credential: Spark APIKey has an invalid format".into());
    }
    let message = format!("host: {HOST}\ndate: {date}\nGET {path} HTTP/1.1");
    let signing_key = hmac::Key::new(hmac::HMAC_SHA256, api_secret);
    let signature = BASE64_STANDARD.encode(hmac::sign(&signing_key, message.as_bytes()).as_ref());
    let authorization = Zeroizing::new(format!(
        "api_key=\"{api_key}\", algorithm=\"hmac-sha256\", headers=\"host date request-line\", signature=\"{signature}\""
    ));
    let encoded = Zeroizing::new(BASE64_STANDARD.encode(authorization.as_bytes()));
    let mut url = Url::parse(&format!("wss://{HOST}{path}")).expect("fixed Spark WebSocket origin");
    url.query_pairs_mut()
        .append_pair("authorization", encoded.as_str())
        .append_pair("date", date)
        .append_pair("host", HOST);
    Ok(url)
}

fn append_frame(frame: Frame, answer: &mut String) -> Result<bool, String> {
    if frame.header.code != 0 {
        return Err(format!("spark_websocket_response: Spark returned error {}", frame.header.code));
    }
    let mut finished = frame.header.status == Some(2);
    if let Some(choices) = frame.payload.and_then(|payload| payload.choices) {
        finished |= choices.status == Some(2);
        for item in choices.text {
            if answer.len() + item.content.len() > RESPONSE_LIMIT {
                return Err("spark_websocket_response: Spark returned an oversized response".into());
            }
            answer.push_str(&item.content);
        }
    }
    Ok(finished)
}
