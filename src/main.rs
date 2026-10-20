#![cfg_attr(windows, windows_subsystem = "windows")]
mod media;
#[cfg(windows)]
mod ncm;
mod settings;
#[cfg(windows)]
mod tray;
use serde_json::{json, Value};
use settings::Settings;
use std::{
    io::{self, Read, Write},
    net::{Shutdown, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU16, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};
use tungstenite::{protocol::Role, Message, WebSocket};
type Reply = Result<Value, String>;
static MEDIA_FALLBACK: AtomicBool = AtomicBool::new(true);
static DEBUG_PORT: AtomicU16 = AtomicU16::new(9222);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Config {
    web_port: u16,
    debug_port: u16,
    media_fallback: bool,
}
impl Config {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Self, String> {
        let mut config = Self {
            web_port: 17890,
            debug_port: 9222,
            media_fallback: true,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--headless" => {}
                "--disable-media-fallback" => config.media_fallback = false,
                "--port" | "--debug-port" => {
                    let value = args.next().ok_or_else(|| format!("{arg} 缺少端口号"))?;
                    if value.is_empty() || !value.bytes().all(|c| c.is_ascii_digit()) {
                        return Err(format!("{arg} 必须为 1–65535 的整数"));
                    }
                    let port = value
                        .parse::<u16>()
                        .ok()
                        .filter(|p| *p > 0)
                        .ok_or_else(|| format!("{arg} 必须为 1–65535 的整数"))?;
                    if arg == "--port" {
                        config.web_port = port;
                    } else {
                        config.debug_port = port;
                    }
                }
                _ => return Err(format!("未知启动参数：{arg}")),
            }
        }
        if config.web_port == config.debug_port {
            return Err("网页端口与网易云调试端口不能相同".into());
        }
        Ok(config)
    }
}
fn media_only(command: &str, deadline: Instant) -> Reply {
    if Instant::now() >= deadline {
        return Err("请求已过期，未发送媒体键".into());
    }
    media::send(command)?;
    Ok(
        json!({"mediaSent":true,"transport":"windows","controlMessage":"已发送 Windows 媒体键；网易云未连接，无法确认响应或兜底"}),
    )
}
enum Event {
    Call(String, Value, Instant, mpsc::Sender<Reply>),
    Wire(u64, Value),
    Lost(u64),
    Wake,
    Stop(mpsc::Sender<()>),
    Reset,
}
struct App {
    settings: Mutex<Settings>,
    tx: mpsc::Sender<Event>,
    shared: Arc<Shared>,
    token: Arc<String>,
    listener: Mutex<Arc<Server>>,
    stopping: AtomicBool,
    ncm_busy: AtomicBool,
    headless: bool,
}
impl App {
    fn save(self: &Arc<Self>, next: Settings) -> Result<String, String> {
        let mut current = self.settings.lock().unwrap();
        let new_listener = if current.web_port != next.web_port {
            Some(Arc::new(
                Server::http(("127.0.0.1", next.web_port))
                    .map_err(|_| "网页端口被占用".to_string())?,
            ))
        } else {
            None
        };
        next.save()?;
        let reset = current.debug_port != next.debug_port;
        MEDIA_FALLBACK.store(next.fallback, Ordering::Relaxed);
        DEBUG_PORT.store(next.debug_port, Ordering::Relaxed);
        *current = next;
        let url = format!("http://127.0.0.1:{}/settings", current.web_port);
        if let Some(server) = new_listener {
            let old = std::mem::replace(&mut *self.listener.lock().unwrap(), server.clone());
            let app = self.clone();
            thread::spawn(move || http_loop(server, app));
            old.unblock();
        }
        if reset {
            let _ = self.tx.send(Event::Reset);
        }
        Ok(url)
    }
    fn shutdown(&self) {
        if self.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        let (tx, rx) = mpsc::channel();
        let _ = self.tx.send(Event::Stop(tx));
        let _ = rx.recv_timeout(Duration::from_secs(5));
        #[cfg(windows)]
        if !self.headless {
            tray::quit();
            return;
        }
        std::process::exit(0);
    }
}
struct Shared {
    state: Mutex<Value>,
    listeners: Mutex<Vec<mpsc::SyncSender<String>>>,
    active: AtomicUsize,
}
impl Shared {
    fn publish(&self, value: Value) {
        let mut state = self.state.lock().unwrap();
        if *state == value {
            return;
        }
        *state = value.clone();
        let body = value.to_string();
        self.listeners
            .lock()
            .unwrap()
            .retain(|tx| match tx.try_send(body.clone()) {
                Ok(()) => true,
                Err(mpsc::TrySendError::Full(_)) => true,
                Err(_) => false,
            });
    }
}
struct Cdp {
    ws: WebSocket<TcpStream>,
    generation: u64,
    id: u64,
}
impl Drop for Cdp {
    fn drop(&mut self) {
        let _ = self.ws.get_ref().shutdown(Shutdown::Both);
    }
}
fn discover(debug_port: u16) -> Result<String, String> {
    let mut stream =
        TcpStream::connect_timeout(&([127, 0, 0, 1], debug_port).into(), Duration::from_secs(2))
            .map_err(|_| {
                format!("无法连接网易云：请以 --remote-debugging-port={debug_port} 启动客户端")
            })?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;
    stream
        .write_all(format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{debug_port}\r\nConnection: close\r\n\r\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    let targets = loop {
        let n = stream.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("调试接口意外关闭".into());
        }
        response.extend_from_slice(&buf[..n]);
        if response.len() > 1024 * 1024 {
            return Err("调试响应过大".into());
        }
        if let Some(offset) = response.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&response[..offset]);
            let length = head
                .lines()
                .find_map(|line| {
                    line.split_once(':')
                        .filter(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
                        .and_then(|(_, v)| v.trim().parse::<usize>().ok())
                })
                .ok_or("调试响应缺少长度")?;
            if response.len() >= offset + 4 + length {
                break serde_json::from_slice::<Value>(&response[offset + 4..offset + 4 + length])
                    .map_err(|e| e.to_string())?;
            }
        }
    };
    let target = targets
        .as_array()
        .ok_or("调试目标列表无效")?
        .iter()
        .find(|v| v["type"] == "page" && v["url"].as_str().unwrap_or("").starts_with("orpheus://"))
        .ok_or("未找到网易云主页面")?;
    let url = target["webSocketDebuggerUrl"]
        .as_str()
        .ok_or("主页面没有调试地址")?;
    if !valid_debug_url(url, debug_port) {
        return Err("拒绝非本机调试地址".into());
    }
    Ok(url.into())
}
fn valid_debug_url(url: &str, port: u16) -> bool {
    url.starts_with(&format!("ws://127.0.0.1:{port}/devtools/page/"))
        || url.starts_with(&format!("ws://localhost:{port}/devtools/page/"))
}
fn connect(tx: mpsc::Sender<Event>, generation: u64, debug_port: u16) -> Result<Cdp, String> {
    let url = discover(debug_port)?;
    let tcp =
        TcpStream::connect_timeout(&([127, 0, 0, 1], debug_port).into(), Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
    tcp.set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    tcp.set_write_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let (ws, _) =
        tungstenite::client(url.as_str(), tcp).map_err(|e| format!("调试连接失败：{e}"))?;
    let read = ws.get_ref().try_clone().map_err(|e| e.to_string())?;
    read.set_read_timeout(None).map_err(|e| e.to_string())?;
    thread::spawn(move || {
        let mut reader = WebSocket::from_raw_socket(read, Role::Client, None);
        while let Ok(msg) = reader.read() {
            match msg {
                Message::Text(s) => {
                    if let Ok(v) = serde_json::from_str::<Value>(&s) {
                        if v.get("id").is_none()
                            && v["method"] != "Runtime.bindingCalled"
                            && v["method"] != "Runtime.executionContextsCleared"
                        {
                            continue;
                        }
                        if tx.send(Event::Wire(generation, v)).is_err() {
                            break;
                        }
                    }
                }
                Message::Close(_) => break,
                _ => {
                    let _ = reader.flush();
                }
            }
        }
        let _ = tx.send(Event::Lost(generation));
    });
    Ok(Cdp {
        ws,
        generation,
        id: 0,
    })
}
fn handle_wire(v: &Value, shared: &Shared) {
    if v["method"] == "Runtime.bindingCalled" && v["params"]["name"] == "__ncmOverlayControlNotify"
    {
        if let Some(s) = v["params"]["payload"].as_str() {
            if let Ok(state) = serde_json::from_str(s) {
                shared.publish(state);
            }
        }
    }
}
fn rpc(
    cdp: &mut Cdp,
    rx: &mpsc::Receiver<Event>,
    pending: &mut Vec<Event>,
    shared: &Shared,
    method: &str,
    params: Value,
) -> Reply {
    cdp.id += 1;
    let id = cdp.id;
    cdp.ws
        .send(Message::text(
            json!({"id":id,"method":method,"params":params}).to_string(),
        ))
        .map_err(|e| e.to_string())?;
    let until = Instant::now() + Duration::from_secs(4);
    loop {
        let event = rx
            .recv_timeout(until.saturating_duration_since(Instant::now()))
            .map_err(|_| "网易云操作超时；该操作不会自动重试".to_string())?;
        match event {
            Event::Wire(g, v) if g == cdp.generation => {
                handle_wire(&v, shared);
                if v["id"] == id {
                    if let Some(err) = v.get("error") {
                        return Err(err.to_string());
                    }
                    if let Some(err) = v["result"].get("exceptionDetails") {
                        return Err(err["exception"]["description"]
                            .as_str()
                            .unwrap_or("客户端执行失败")
                            .into());
                    }
                    return Ok(v["result"].clone());
                }
            }
            Event::Lost(g) if g == cdp.generation => {
                return Err("网易云连接已断开；请稍后重新操作".into())
            }
            other => pending.push(other),
        }
    }
}
fn evaluate(
    cdp: &mut Cdp,
    rx: &mpsc::Receiver<Event>,
    pending: &mut Vec<Event>,
    shared: &Shared,
    expression: &str,
) -> Reply {
    let result = rpc(
        cdp,
        rx,
        pending,
        shared,
        "Runtime.evaluate",
        json!({"expression":expression,"returnByValue":true,"timeout":3500,"awaitPromise":true}),
    )?;
    result["result"]
        .get("value")
        .cloned()
        .ok_or("播放器返回了无效状态".into())
}
fn initialize(
    cdp: &mut Cdp,
    rx: &mpsc::Receiver<Event>,
    pending: &mut Vec<Event>,
    shared: &Shared,
) -> Reply {
    rpc(cdp, rx, pending, shared, "Runtime.enable", json!({}))?;
    rpc(
        cdp,
        rx,
        pending,
        shared,
        "Runtime.addBinding",
        json!({"name":"__ncmOverlayControlNotify"}),
    )?;
    evaluate(cdp, rx, pending, shared, include_str!("../web/adapter.js"))
}
fn worker(
    rx: mpsc::Receiver<Event>,
    tx: mpsc::Sender<Event>,
    shared: Arc<Shared>,
    mut debug_port: u16,
) {
    let mut cdp: Option<Cdp> = None;
    let mut generation = 0;
    let mut pending = Vec::new();
    let mut tick = Instant::now() + Duration::from_secs(20);
    loop {
        let event = if shared.active.load(Ordering::Relaxed) > 0 && Instant::now() >= tick {
            tick = Instant::now() + Duration::from_secs(20);
            Event::Wake
        } else if !pending.is_empty() {
            pending.remove(0)
        } else if shared.active.load(Ordering::Relaxed) > 0 {
            match rx.recv_timeout(tick.saturating_duration_since(Instant::now())) {
                Ok(e) => e,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    tick = Instant::now() + Duration::from_secs(20);
                    Event::Wake
                }
                Err(_) => break,
            }
        } else {
            match rx.recv() {
                Ok(e) => e,
                Err(_) => break,
            }
        };
        match event {
            Event::Reset => {
                if let Some(mut c) = cdp.take() {
                    let _ = evaluate(
                        &mut c,
                        &rx,
                        &mut pending,
                        &shared,
                        "(window.__ncmOverlayControlBridge?.dispose(),true)",
                    );
                }
                debug_port = DEBUG_PORT.load(Ordering::Relaxed);
                shared.publish(json!({"connected":false,"error":"连接设置已更新"}));
                if shared.active.load(Ordering::Relaxed) > 0 {
                    let _ = tx.send(Event::Wake);
                }
            }
            Event::Stop(reply) => {
                if let Some(mut c) = cdp.take() {
                    let _ = evaluate(
                        &mut c,
                        &rx,
                        &mut pending,
                        &shared,
                        "(window.__ncmOverlayControlBridge?.dispose(),true)",
                    );
                }
                let _ = reply.send(());
                break;
            }
            Event::Wire(g, v) => {
                if cdp.as_ref().is_some_and(|c| c.generation == g) {
                    if v["method"] == "Runtime.executionContextsCleared" {
                        cdp = None;
                        shared.publish(
                            json!({"connected":false,"error":"网易云页面重载，等待重新连接"}),
                        );
                        continue;
                    }
                    handle_wire(&v, &shared);
                }
            }
            Event::Lost(g) => {
                if cdp.as_ref().is_some_and(|c| c.generation == g) {
                    cdp = None;
                    shared.publish(json!({"connected":false,"error":"网易云已断开，正在等待重连"}));
                }
            }
            Event::Wake if shared.active.load(Ordering::Relaxed) == 0 => {
                if let Some(mut c) = cdp.take() {
                    let _ = evaluate(
                        &mut c,
                        &rx,
                        &mut pending,
                        &shared,
                        "(window.__ncmOverlayControlBridge?.dispose(),true)",
                    );
                }
            }
            e => {
                let call = if let Event::Call(cmd, value, deadline, reply) = e {
                    if Instant::now() >= deadline {
                        let _ = reply.send(Err("请求已过期，未执行".into()));
                        continue;
                    }
                    Some((cmd, value, deadline, reply))
                } else {
                    None
                };
                if cdp.is_none() {
                    generation += 1;
                    match connect(tx.clone(), generation, debug_port) {
                        Ok(mut c) => match initialize(&mut c, &rx, &mut pending, &shared) {
                            Ok(v) => {
                                shared.publish(v);
                                cdp = Some(c);
                            }
                            Err(err) => {
                                shared.publish(json!({"connected":false,"error":err.clone()}));
                                if let Some((cmd, value, deadline, reply)) = call {
                                    let result = if cmd == "__windows" {
                                        media_only(value.as_str().unwrap(), deadline)
                                    } else {
                                        Err(err)
                                    };
                                    let _ = reply.send(result);
                                }
                                continue;
                            }
                        },
                        Err(err) => {
                            shared.publish(json!({"connected":false,"error":err.clone()}));
                            if let Some((cmd, value, deadline, reply)) = call {
                                let result = if cmd == "__windows" {
                                    media_only(value.as_str().unwrap(), deadline)
                                } else {
                                    Err(err)
                                };
                                let _ = reply.send(result);
                            }
                            continue;
                        }
                    }
                }
                let c = cdp.as_mut().unwrap();
                if let Some((_, _, deadline, reply)) = &call {
                    if Instant::now() >= *deadline {
                        let _ = reply.send(Err("连接耗时过长，操作未执行，请重新操作".into()));
                        continue;
                    }
                }
                let expression = if let Some((cmd, value, deadline, reply)) = &call {
                    if cmd == "__windows" {
                        // Fresh CDP read, not a possibly stale browser/SSE snapshot.
                        let before = evaluate(
                            c,
                            &rx,
                            &mut pending,
                            &shared,
                            "window.__ncmOverlayControlBridge.snapshot()",
                        );
                        match before {
                            Ok(before) => {
                                if Instant::now() >= *deadline {
                                    let _ = reply.send(Err("请求已过期，未发送媒体键".into()));
                                    continue;
                                }
                                let send_error = media::send(value.as_str().unwrap()).err();
                                format!(
                                    "window.__ncmOverlayControlBridge.finishMedia({},{},{})",
                                    value,
                                    before,
                                    json!(send_error)
                                )
                            }
                            Err(_) => {
                                if let Some((_, _, _, reply)) = &call {
                                    let _ =
                                        reply.send(media_only(value.as_str().unwrap(), *deadline));
                                }
                                cdp = None;
                                continue;
                            }
                        }
                    } else {
                        format!(
                            "window.__ncmOverlayControlBridge.command({},{})",
                            json!(cmd),
                            value
                        )
                    }
                } else {
                    "(window.__ncmOverlayControlBridge?.touch(),window.__ncmOverlayControlBridge?.snapshot())"
                        .into()
                };
                let result = evaluate(c, &rx, &mut pending, &shared, &expression);
                match &result {
                    Ok(v) => shared.publish(v.clone()),
                    Err(err) => {
                        cdp = None;
                        shared.publish(json!({"connected":false,"error":err}));
                    }
                }
                if let Some((_, _, _, reply)) = call {
                    let _ = reply.send(result);
                }
                if shared.active.load(Ordering::Relaxed) == 0 {
                    let _ = tx.send(Event::Wake);
                }
            }
        }
    }
}
fn valid_command(v: &Value) -> bool {
    if let Some(transport) = v.get("transport") {
        if !matches!(transport.as_str(), Some("windows" | "ncm")) {
            return false;
        }
        if transport == "windows" && !media::supported(v["command"].as_str().unwrap_or("")) {
            return false;
        }
    }
    match v["command"].as_str().unwrap_or("") {
        "toggle" | "previous" | "next" | "like" | "dislike" | "dislikeCancel" | "mute"
        | "shutdown" => true,
        "dislikeReason" => v["value"].as_u64().is_some_and(|n| n < 20),
        "volume" => v["value"].as_u64().is_some_and(|n| n <= 100),
        "mode" => [
            "playOrder",
            "playCycle",
            "playOneCycle",
            "playRandom",
            "playAi",
            "playFm",
        ]
        .contains(&v["value"].as_str().unwrap_or("")),
        _ => false,
    }
}
fn header(r: &Request, key: &'static str) -> String {
    r.headers()
        .iter()
        .find(|h| h.field.equiv(key))
        .map(|h| h.value.as_str().to_string())
        .unwrap_or_default()
}
fn authorized(host: &str, origin: &str, port: u16) -> bool {
    (host == format!("127.0.0.1:{port}") || host == format!("localhost:{port}"))
        && (origin.is_empty()
            || origin == format!("http://127.0.0.1:{port}")
            || origin == format!("http://localhost:{port}"))
}
fn response(r: Request, status: u16, kind: &str, body: String) {
    let mut res = Response::from_string(body).with_status_code(StatusCode(status));
    for (k,v) in [("Content-Type",kind),("Cache-Control","no-store"),("X-Content-Type-Options","nosniff"),("Content-Security-Policy","default-src 'self'; style-src 'self'; script-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'"),("Referrer-Policy","no-referrer")]{res.add_header(Header::from_bytes(k,v).unwrap());}
    let _ = r.respond(res);
}
struct Events {
    rx: mpsc::Receiver<String>,
    bytes: io::Cursor<Vec<u8>>,
    shared: Arc<Shared>,
    tx: mpsc::Sender<Event>,
}
impl Read for Events {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.bytes.position() as usize == self.bytes.get_ref().len() {
            let body = match self.rx.recv_timeout(Duration::from_secs(15)) {
                Ok(_) => format!("data: {}\n\n", self.shared.state.lock().unwrap()),
                Err(mpsc::RecvTimeoutError::Timeout) => ": heartbeat\n\n".into(),
                Err(_) => return Ok(0),
            };
            self.bytes = io::Cursor::new(body.into_bytes());
        }
        self.bytes.read(buf)
    }
}
impl Drop for Events {
    fn drop(&mut self) {
        self.shared.active.fetch_sub(1, Ordering::Relaxed);
        let _ = self.tx.send(Event::Wake);
    }
}
fn serve(
    mut r: Request,
    tx: mpsc::Sender<Event>,
    shared: Arc<Shared>,
    token: Arc<String>,
    config: Config,
    app: Arc<App>,
) {
    if !authorized(&header(&r, "Host"), &header(&r, "Origin"), config.web_port) {
        response(r, 403, "text/plain; charset=utf-8", "拒绝跨站访问".into());
        return;
    }
    let route = r.url().to_string();
    let path = route.split('?').next().unwrap_or("");
    if r.method() == &Method::Get {
        if path == "/api/ncm-status" {
            #[cfg(windows)]
            let running = !ncm::processes().is_empty();
            #[cfg(not(windows))]
            let running = false;
            response(
                r,
                200,
                "application/json",
                json!({"running":running}).to_string(),
            );
            return;
        }
        if path == "/api/settings" {
            response(
                r,
                200,
                "application/json",
                app.settings.lock().unwrap().json().to_string(),
            );
            return;
        }
        if path == "/licenses" {
            response(
                r,
                200,
                "text/plain; charset=utf-8",
                include_str!(concat!(env!("OUT_DIR"), "/licenses.txt")).into(),
            );
            return;
        }
        let asset = match path {
            "/" => Some((
                "text/html; charset=utf-8",
                include_str!("../web/index.html"),
            )),
            "/app.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../web/app.js"),
            )),
            "/style.css" => Some(("text/css; charset=utf-8", include_str!("../web/style.css"))),
            "/tokens.css" => Some(("text/css; charset=utf-8", include_str!("../web/tokens.css"))),
            "/settings" => Some((
                "text/html; charset=utf-8",
                include_str!("../web/settings.html"),
            )),
            "/settings.js" => Some((
                "text/javascript; charset=utf-8",
                include_str!("../web/settings.js"),
            )),
            _ => None,
        };
        if let Some((kind, body)) = asset {
            response(r, 200, kind, body.into());
            return;
        }
        if path == "/session" {
            response(
                r,
                200,
                "application/json",
                json!({"token":*token,"webPort":config.web_port,"debugPort":config.debug_port,"fallback":config.media_fallback}).to_string(),
            );
            return;
        }
        if path == "/events" && route == format!("/events?token={token}") {
            if shared.active.fetch_add(1, Ordering::Relaxed) >= 8 {
                shared.active.fetch_sub(1, Ordering::Relaxed);
                response(r, 429, "text/plain", "最多同时打开 8 个控件".into());
                return;
            }
            let (sx, rx) = mpsc::sync_channel(8);
            let initial = shared.state.lock().unwrap().to_string();
            let _ = sx.try_send(initial);
            {
                let initial = shared.state.lock().unwrap().to_string();
                let mut listeners = shared.listeners.lock().unwrap();
                listeners.retain(|tx| {
                    !matches!(
                        tx.try_send(initial.clone()),
                        Err(mpsc::TrySendError::Disconnected(_))
                    )
                });
                listeners.push(sx);
            }
            let _ = tx.send(Event::Wake);
            let mut reader = Events {
                rx,
                bytes: io::Cursor::new(Vec::new()),
                shared,
                tx,
            };
            // Flush each event: generic response streaming buffers small writes.
            let mut writer = r.into_writer();
            if writer.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\nConnection: close\r\nX-Content-Type-Options: nosniff\r\n\r\n").and_then(|_|writer.flush()).is_ok() {
                let mut buffer=[0u8;4096];
                while let Ok(n)=reader.read(&mut buffer) {if n==0 || writer.write_all(&buffer[..n]).and_then(|_|writer.flush()).is_err(){break;}}
            }
            return;
        }
    }
    if r.method() == &Method::Post
        && ["/api/settings", "/api/detect", "/api/connect"].contains(&path)
    {
        if header(&r, "X-Overlay-Token") != *token {
            response(
                r,
                403,
                "application/json",
                json!({"error":"会话失效，请刷新"}).to_string(),
            );
            return;
        }
        let mut body = String::new();
        if r.body_length().is_some_and(|n| n > 4096)
            || r.as_reader().take(4097).read_to_string(&mut body).is_err()
            || body.len() > 4096
        {
            response(r, 413, "application/json", "{}".into());
            return;
        }
        let result: Result<Value, String> = match path {
            "/api/settings" => serde_json::from_str::<Value>(&body)
                .map_err(|_| "设置格式无效".to_string())
                .and_then(|v| Settings::parse(&v))
                .and_then(|next| app.save(next))
                .map(|url| json!({"ok":true,"url":url})),
            "/api/detect" => {
                #[cfg(windows)]
                {
                    ncm::detect().map(|path| json!({"path":path}))
                }
                #[cfg(not(windows))]
                {
                    Err("仅支持 Windows".into())
                }
            }
            _ => {
                if app.ncm_busy.swap(true, Ordering::SeqCst) {
                    Err("正在连接，请稍候".into())
                } else {
                    let mut settings = app.settings.lock().unwrap().clone();
                    #[cfg(windows)]
                    let result = (|| {
                        if settings.ncm_path.is_empty() {
                            settings.ncm_path = ncm::detect()?;
                        }
                        ncm::connect(&settings, false).map(|_| json!({"ok":true}))
                    })();
                    #[cfg(not(windows))]
                    let result: Result<Value, String> = Err("仅支持 Windows".into());
                    app.ncm_busy.store(false, Ordering::SeqCst);
                    result
                }
            }
        };
        match result {
            Ok(value) => response(r, 200, "application/json", value.to_string()),
            Err(error) => response(
                r,
                400,
                "application/json",
                json!({"error":error}).to_string(),
            ),
        };
        return;
    }
    if r.method() == &Method::Post && path == "/command" {
        if header(&r, "X-Overlay-Token") != *token {
            response(
                r,
                403,
                "application/json",
                json!({"error":"会话失效，请刷新页面"}).to_string(),
            );
            return;
        }
        if r.body_length().is_some_and(|n| n > 1024) {
            response(r, 413, "application/json", "{}".into());
            return;
        }
        let mut body = String::new();
        if r.as_reader().take(1025).read_to_string(&mut body).is_err() || body.len() > 1024 {
            response(r, 400, "application/json", "{}".into());
            return;
        }
        let v: Value = serde_json::from_str(&body).unwrap_or(Value::Null);
        if !valid_command(&v) {
            response(
                r,
                400,
                "application/json",
                json!({"error":"无效操作或参数"}).to_string(),
            );
            return;
        }
        if v["transport"] == "windows" {
            if MEDIA_FALLBACK.load(Ordering::Relaxed) {
                let (sx, rx) = mpsc::channel();
                let _ = tx.send(Event::Call(
                    "__windows".into(),
                    v["command"].clone(),
                    Instant::now() + Duration::from_secs(9),
                    sx,
                ));
                match rx.recv_timeout(Duration::from_secs(10)) {
                    Ok(Ok(value)) => response(r, 200, "application/json", value.to_string()),
                    Ok(Err(error)) => response(
                        r,
                        502,
                        "application/json",
                        json!({"error":error}).to_string(),
                    ),
                    Err(_) => response(
                        r,
                        504,
                        "application/json",
                        json!({"error":"媒体控制超时，未自动重试，请检查播放器状态"}).to_string(),
                    ),
                }
                return;
            }
            match media::send(v["command"].as_str().unwrap()) {
                Ok(()) => response(
                    r,
                    200,
                    "application/json",
                    json!({"mediaSent":true,"transport":"windows"}).to_string(),
                ),
                Err(error) => response(
                    r,
                    502,
                    "application/json",
                    json!({"error":error}).to_string(),
                ),
            }
            return;
        }
        if v["command"] == "shutdown" {
            response(
                r,
                200,
                "application/json",
                json!({"connected":false,"error":"控件已退出，网易云仍正常播放"}).to_string(),
            );
            app.shutdown();
            return;
        }
        let (sx, rx) = mpsc::channel();
        let _ = tx.send(Event::Call(
            v["command"].as_str().unwrap().into(),
            v["value"].clone(),
            Instant::now() + Duration::from_secs(9),
            sx,
        ));
        match rx.recv_timeout(Duration::from_secs(10)) {
            Ok(Ok(state)) => response(r, 200, "application/json", state.to_string()),
            Ok(Err(error)) => response(
                r,
                502,
                "application/json",
                json!({"error":error}).to_string(),
            ),
            Err(_) => response(
                r,
                504,
                "application/json",
                json!({"error":"操作超时，不会自动重复执行"}).to_string(),
            ),
        }
        return;
    }
    response(r, 404, "text/plain", "Not found".into());
}
fn main() {
    let headless = std::env::args().any(|s| s == "--headless");
    #[cfg(windows)]
    let _instance = if !headless {
        match tray::single_instance() {
            Ok(Some(instance)) => Some(instance),
            Ok(None) => return,
            Err(error) => {
                tray::fatal(&error);
                return;
            }
        }
    } else {
        None
    };
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut settings = Settings::load().unwrap_or_default();
    let config = match Config::parse(args.clone()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(2);
        }
    };
    if args.iter().any(|s| s == "--port") {
        settings.web_port = config.web_port;
    }
    if args.iter().any(|s| s == "--debug-port") {
        settings.debug_port = config.debug_port;
    }
    if args.iter().any(|s| s == "--disable-media-fallback") {
        settings.fallback = false;
    }
    if settings.web_port == settings.debug_port {
        eprintln!("两个端口不能相同");
        return;
    }
    MEDIA_FALLBACK.store(settings.fallback, Ordering::Relaxed);
    DEBUG_PORT.store(settings.debug_port, Ordering::Relaxed);
    let server = match Server::http(("127.0.0.1", settings.web_port)) {
        Ok(s) => s,
        Err(e) => {
            if headless {
                eprintln!("{e}");
                std::process::exit(1);
            }
            match Server::http(("127.0.0.1", 0)) {
                Ok(server) => {
                    settings.web_port = server.server_addr().to_ip().unwrap().port();
                    server
                }
                Err(error) => {
                    #[cfg(windows)]
                    tray::fatal(&error.to_string());
                    return;
                }
            }
        }
    };
    let token = Arc::new(format!("{:032x}", rand::random::<u128>()));
    let shared = Arc::new(Shared {
        state: Mutex::new(json!({"connected":false,"error":"等待连接网易云"})),
        listeners: Mutex::new(Vec::new()),
        active: AtomicUsize::new(0),
    });
    let (tx, rx) = mpsc::channel();
    let worker_state = shared.clone();
    let worker_tx = tx.clone();
    let debug_port = settings.debug_port;
    thread::spawn(move || worker(rx, worker_tx, worker_state, debug_port));
    let app = Arc::new(App {
        settings: Mutex::new(settings.clone()),
        tx,
        shared,
        token,
        listener: Mutex::new(Arc::new(server)),
        stopping: AtomicBool::new(false),
        ncm_busy: AtomicBool::new(false),
        headless,
    });
    #[cfg(windows)]
    if !headless {
        let app = app.clone();
        thread::spawn(move || {
            let mut previous = None;
            while !app.stopping.load(Ordering::Relaxed) {
                let settings = app.settings.lock().unwrap().clone();
                if settings.auto_debug {
                    let pids: Vec<u32> = ncm::processes()
                        .into_iter()
                        .filter(|(_, path)| path.eq_ignore_ascii_case(&settings.ncm_path))
                        .map(|(pid, _)| pid)
                        .collect();
                    let fingerprint =
                        (pids.clone(), settings.ncm_path.clone(), settings.debug_port);
                    if previous.as_ref() != Some(&fingerprint)
                        && !pids.is_empty()
                        && !app.ncm_busy.swap(true, Ordering::SeqCst)
                    {
                        previous = Some(fingerprint);
                        if let Err(error) = ncm::connect(&settings, true) {
                            app.shared.publish(json!({"connected":false,"error":error}));
                        }
                        app.ncm_busy.store(false, Ordering::SeqCst);
                    } else if pids.is_empty() {
                        previous = None;
                    }
                } else {
                    previous = None;
                }
                thread::sleep(Duration::from_secs(3));
            }
        });
    }
    let app_http = app.clone();
    let server = app.listener.lock().unwrap().clone();
    thread::spawn(move || http_loop(server, app_http));
    if headless {
        loop {
            thread::park();
        }
    }
    #[cfg(windows)]
    if let Err(error) = tray::run(app) {
        tray::fatal(&error);
    }
}
fn http_loop(server: Arc<Server>, app: Arc<App>) {
    let port = server.server_addr().to_ip().unwrap().port();
    let connections = Arc::new(AtomicUsize::new(0));
    for r in server.incoming_requests() {
        if connections.load(Ordering::Relaxed) >= 24 {
            response(r, 429, "text/plain", "Too many requests".into());
            continue;
        }
        connections.fetch_add(1, Ordering::Relaxed);
        let count = connections.clone();
        let tx = app.tx.clone();
        let shared = app.shared.clone();
        let token = app.token.clone();
        let app = app.clone();
        let current = app.settings.lock().unwrap().clone();
        let config = Config {
            web_port: port,
            debug_port: current.debug_port,
            media_fallback: current.fallback,
        };
        thread::spawn(move || {
            serve(r, tx, shared, token, config, app);
            count.fetch_sub(1, Ordering::Relaxed);
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn command_allowlist() {
        assert!(media_only("toggle", Instant::now() - Duration::from_secs(1)).is_err());
        for command in ["previous", "next", "toggle"] {
            for transport in ["windows", "ncm"] {
                assert!(valid_command(
                    &json!({"command":command,"transport":transport})
                ));
            }
        }
        assert!(!valid_command(
            &json!({"command":"toggle","transport":"unknown"})
        ));
        assert!(!valid_command(
            &json!({"command":"toggle","transport":null})
        ));
        assert!(!valid_command(
            &json!({"command":"volume","value":50,"transport":"windows"})
        ));
        assert!(!valid_command(
            &json!({"command":"shutdown","transport":"windows"})
        ));
        assert!(valid_command(&json!({"command":"volume","value":0})));
        assert!(valid_command(&json!({"command":"volume","value":100})));
        for value in [json!(-1), json!(101), json!(1.5), json!("50")] {
            assert!(!valid_command(&json!({"command":"volume","value":value})));
        }
        assert!(!valid_command(
            &json!({"command":"evaluate","value":"alert(1)"})
        ));
        assert!(valid_command(&json!({"command":"mode","value":"playFm"})));
    }
    #[test]
    fn origin_checks() {
        assert!(authorized(
            "127.0.0.1:17890",
            "http://127.0.0.1:17890",
            17890
        ));
        assert!(!authorized("evil.example:17890", "", 17890));
        assert!(!authorized("127.0.0.1:17890", "http://evil.example", 17890));
        assert!(!authorized("127.0.0.1:17890", "null", 17890));
        assert!(authorized(
            "localhost:23456",
            "http://localhost:23456",
            23456
        ));
        assert!(!authorized("127.0.0.1:17890", "", 23456));
        assert!(!authorized(
            "127.0.0.1:23456",
            "http://localhost:17890",
            23456
        ));
        assert!(!authorized("127.0.0.1:23456.evil", "", 23456));
    }
    #[test]
    fn port_configuration() {
        let parse = |args: &[&str]| Config::parse(args.iter().map(|s| s.to_string()));
        assert_eq!(
            parse(&[]).unwrap(),
            Config {
                web_port: 17890,
                debug_port: 9222,
                media_fallback: true
            }
        );
        assert_eq!(
            parse(&[
                "--headless",
                "--port",
                "23456",
                "--debug-port",
                "9333",
                "--disable-media-fallback"
            ])
            .unwrap(),
            Config {
                web_port: 23456,
                debug_port: 9333,
                media_fallback: false
            }
        );
        for args in [
            vec!["--port"],
            vec!["--debug-port", "0"],
            vec!["--port", "65536"],
            vec!["--port", "-1"],
            vec!["--port", "1.5"],
            vec!["--port", "+42"],
            vec!["--port", "9222"],
            vec!["--debug-port", "17890"],
            vec!["--unexpected"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
        assert!(parse(&["--port", "1", "--debug-port", "65535"]).is_ok());
    }
    #[test]
    fn debugger_url_stays_on_selected_loopback_port() {
        assert!(valid_debug_url(
            "ws://localhost:9333/devtools/page/ABC",
            9333
        ));
        assert!(valid_debug_url(
            "ws://127.0.0.1:9333/devtools/page/ABC",
            9333
        ));
        for url in [
            "ws://localhost:9222/devtools/page/ABC",
            "ws://evil:9333/devtools/page/ABC",
            "ws://localhost:9333.evil/devtools/page/ABC",
            "ws://127.0.0.1:9333/other",
        ] {
            assert!(!valid_debug_url(url, 9333));
        }
    }
    #[test]
    fn subscriber_dedup_and_cleanup() {
        let shared = Shared {
            state: Mutex::new(Value::Null),
            listeners: Mutex::new(Vec::new()),
            active: AtomicUsize::new(0),
        };
        let (tx, rx) = mpsc::sync_channel(2);
        shared.listeners.lock().unwrap().push(tx);
        shared.publish(json!({"volume":37}));
        assert!(rx.try_recv().is_ok());
        shared.publish(json!({"volume":37}));
        assert!(rx.try_recv().is_err());
        drop(rx);
        shared.publish(json!({"volume":38}));
        assert!(shared.listeners.lock().unwrap().is_empty());
    }
    #[test]
    fn slow_client_gets_latest_state() {
        let shared = Arc::new(Shared {
            state: Mutex::new(json!({"volume":99})),
            listeners: Mutex::new(Vec::new()),
            active: AtomicUsize::new(1),
        });
        let (sx, rx) = mpsc::sync_channel(1);
        sx.send("old state".into()).unwrap();
        let (tx, _) = mpsc::channel();
        let mut reader = Events {
            rx,
            bytes: io::Cursor::new(Vec::new()),
            shared: shared.clone(),
            tx,
        };
        let mut bytes = [0u8; 128];
        let n = reader.read(&mut bytes).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes[..n]).unwrap(),
            "data: {\"volume\":99}\n\n"
        );
        drop(reader);
        assert_eq!(shared.active.load(Ordering::Relaxed), 0);
    }
}
