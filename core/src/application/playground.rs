//! Browser UI for comparing saved Models. All inference runs in Rust using
//! Arca's existing Provider and credential settings.
use std::{
    io::Read,
    net::{Ipv4Addr, SocketAddrV4},
    sync::{atomic::{AtomicBool, AtomicUsize, Ordering}, Arc},
    thread,
    time::Duration,
};

use serde::Serialize;
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use uuid::Uuid;

use crate::resources::model::inference::{self, Failure};

const WORKERS: usize = 8;
const MAX_PARALLEL: usize = 6;
const MAX_BODY: u64 = 131_072;
const TOKEN_HEADER: &str = "X-Arca-Playground";
const WEB_ASSETS: &[WebAsset] = include!(concat!(env!("OUT_DIR"), "/playground_assets.rs"));

struct WebAsset {
    url: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

pub struct Playground {
    origin: String,
    token: String,
    state: Arc<State>,
}

struct State {
    host: String,
    origin: String,
    token: String,
    stopping: AtomicBool,
    active: AtomicUsize,
}

impl Playground {
    pub fn start() -> Result<Self, String> {
        if !WEB_ASSETS.iter().any(|asset| asset.mime.starts_with("text/javascript")) {
            return Err("此构建未包含模型试验台。请先在仓库根目录运行 npm --prefix playground ci 和 npm --prefix playground run build，再重新构建 Arca。".into());
        }
        let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let server = Arc::new(Server::http(address).map_err(|_| "无法启动本机模型试验台。".to_owned())?);
        let address = server.server_addr().to_ip().ok_or_else(|| "模型试验台缺少本机地址。".to_owned())?;
        let origin = format!("http://{address}");
        let token = Uuid::new_v4().simple().to_string();
        let state = Arc::new(State {
            host: address.to_string(),
            origin: origin.clone(),
            token: token.clone(),
            stopping: AtomicBool::new(false),
            active: AtomicUsize::new(0),
        });
        let playground = Self { origin, token, state };
        for index in 0..WORKERS {
            let state = Arc::clone(&playground.state);
            let server = Arc::clone(&server);
            thread::Builder::new().name(format!("playground-{index}"))
                .spawn(move || {
                    while !state.stopping.load(Ordering::Relaxed) {
                        match server.recv_timeout(Duration::from_millis(300)) {
                            Ok(Some(request)) => {
                                let _ = handle(request, &state);
                            }
                            Ok(None) => {}
                            Err(_) => break,
                        }
                    }
                })
                .map_err(|_| "无法创建试验台请求处理线程。".to_owned())?;
        }
        Ok(playground)
    }

    /// Only browsers given this local URL can make API calls. The fragment is
    /// not sent in HTTP requests or logged by the server.
    pub fn launch_url(&self) -> String {
        format!("{}/#{}", self.origin, self.token)
    }
}

impl Drop for Playground {
    fn drop(&mut self) {
        self.state.stopping.store(true, Ordering::Relaxed);
    }
}

impl State {
    fn claim(&self) -> Result<RunSlot<'_>, Failure> {
        self.active.try_update(Ordering::AcqRel, Ordering::Relaxed, |running| {
            (running < MAX_PARALLEL).then_some(running + 1)
        }).map_err(|_| Failure::new(429, "playground_busy", "并发请求已满，请等待一条请求完成。"))?;
        Ok(RunSlot(&self.active))
    }
}

struct RunSlot<'a>(&'a AtomicUsize);

impl Drop for RunSlot<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Release);
    }
}

fn handle(mut request: Request, state: &State) -> std::io::Result<()> {
    // Rebinding a remote hostname to loopback gives it the wrong Host even
    // before the API's origin and unguessable per-launch token checks.
    if header(&request, "Host").ok().flatten() != Some(state.host.as_str()) {
        return respond_error(request, Failure::new(403, "invalid_host", "请使用 Arca 打开的本机地址。"));
    }
    if request.method() == &Method::Get {
        if let Some(asset) = WEB_ASSETS.iter().find(|asset| asset.url == request.url()) {
            return respond(request, 200, asset.mime, asset.bytes);
        }
    }
    if !matches!(request.url(), "/api/models" | "/api/run") {
        return respond_error(request, Failure::new(404, "not_found", "这个入口不存在。"));
    }
    let valid_origin = match header(&request, "Origin") {
        Ok(Some(origin)) => origin == state.origin,
        Ok(None) => request.method() == &Method::Get,
        Err(()) => false,
    };
    if !valid_origin || header(&request, TOKEN_HEADER).ok().flatten() != Some(state.token.as_str()) {
        return respond_error(request, Failure::new(403, "playground_session", "请从 Arca 重新打开模型试验台。"));
    }
    match (request.method(), request.url()) {
        (&Method::Get, "/api/models") => {
            let targets = super::gate::read(inference::targets);
            respond_result(request, targets)
        }
        (&Method::Post, "/api/run") => {
            let result = run(&mut request, state);
            respond_result(request, result)
        }
        _ => respond_error(request, Failure::new(405, "invalid_method", "请求方式无效。")),
    }
}

fn run(request: &mut Request, state: &State) -> Result<inference::Completed, Failure> {
    let _slot = state.claim()?;
    if !header(request, "Content-Type").ok().flatten().is_some_and(|mime| {
        mime.split(';').next().is_some_and(|part| part.trim().eq_ignore_ascii_case("application/json"))
    }) {
        return Err(Failure::new(415, "invalid_content_type", "请求必须使用 JSON。"));
    }
    if request.body_length().is_some_and(|len| len as u64 > MAX_BODY) {
        return Err(Failure::new(413, "request_too_large", "请求太长，请缩短文本或高级参数。"));
    }
    let mut body = Vec::new();
    request.as_reader().take(MAX_BODY + 1).read_to_end(&mut body)
        .map_err(|_| Failure::new(400, "invalid_request", "无法读取试验请求。"))?;
    if body.len() as u64 > MAX_BODY {
        return Err(Failure::new(413, "request_too_large", "请求太长，请缩短文本或高级参数。"));
    }
    let input = serde_json::from_slice(&body)
        .map_err(|_| Failure::new(400, "invalid_request", "参数格式有误，请检查数字和高级 JSON。"))?;
    let prepared = super::gate::read(|| inference::prepare(input))?;
    prepared.execute()
}

fn header<'a>(request: &'a Request, name: &'static str) -> Result<Option<&'a str>, ()> {
    let mut matches = request.headers().iter().filter(|header| header.field.equiv(name));
    let first = matches.next().map(|header| header.value.as_str());
    if matches.next().is_some() { Err(()) } else { Ok(first) }
}

fn respond_result(request: Request, result: Result<impl Serialize, Failure>) -> std::io::Result<()> {
    match result {
        Ok(value) => match serde_json::to_string(&value) {
            Ok(body) => respond(request, 200, "application/json", body),
            Err(_) => respond_error(request, Failure::new(500, "serialization_failed", "无法显示试验结果。")),
        },
        Err(error) => respond_error(request, error),
    }
}

fn respond_error(request: Request, error: Failure) -> std::io::Result<()> {
    respond(request, error.status, "application/json", serde_json::json!({"error": error}).to_string())
}

fn respond(request: Request, status: u16, content_type: &str, body: impl Into<Vec<u8>>) -> std::io::Result<()> {
    let mut response = Response::from_data(body).with_status_code(StatusCode(status));
    for (name, value) in [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("Referrer-Policy", "no-referrer"),
        ("X-Content-Type-Options", "nosniff"),
        ("Content-Security-Policy", "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"),
    ] {
        if let Ok(header) = Header::from_bytes(name.as_bytes(), value.as_bytes()) {
            response.add_header(header);
        }
    }
    request.respond(response)
}
