use crate::model::{Camera, Ticket, request};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::{JsFuture, spawn_local};
use web_sys::{
    HtmlVideoElement, RequestInit, RequestRedirect, Response, RtcConfiguration, RtcIceServer,
    RtcPeerConnection, RtcRtpTransceiverDirection, RtcRtpTransceiverInit, RtcSdpType,
    RtcSessionDescriptionInit,
};
use xcss_web::component::{component_props_builder, component_view};
use xcss_web::i18n::t;
use xcss_web::{html, prelude::*};

#[wasm_bindgen(module = "/src/hls-binding.js")]
extern "C" {
    #[wasm_bindgen(catch,js_name=createHls)]
    fn create_hls(
        video: &HtmlVideoElement,
        url: &str,
        token: &str,
        manifest: &js_sys::Function,
        failure: &js_sys::Function,
    ) -> Result<js_sys::Promise, JsValue>;
    #[wasm_bindgen(js_name=destroyHls)]
    fn destroy_hls(hls: &JsValue);
}

pub fn media_url(value: &str, base: &str) -> Result<String, JsValue> {
    let page = web_sys::Url::new(base)?;
    let url = web_sys::Url::new_with_base(value, base)?;
    if page.origin() != url.origin() || !url.username().is_empty() || !url.password().is_empty() {
        return Err(JsValue::from_str(
            "Media URL must use the application origin",
        ));
    }
    Ok(url.href())
}
pub fn resource_url(location: &str, request_url: &str, page: &str) -> Result<String, JsValue> {
    let request = media_url(request_url, page)?;
    let location = if location.starts_with('/')
        && !location.starts_with("//")
        && web_sys::Url::new(&request)?
            .pathname()
            .starts_with("/media-webrtc/")
    {
        format!("/media-webrtc{location}")
    } else {
        location.into()
    };
    media_url(&location, &request)
}
type IceServer = (String, Option<String>, Option<String>);
fn parse_ice_servers(header: &str) -> Vec<IceServer> {
    let mut entries = Vec::new();
    let mut start = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (index, byte) in header.bytes().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' && quoted {
            escaped = true;
            continue;
        }
        if byte == b'"' {
            quoted = !quoted;
        }
        if byte == b',' && !quoted && header[index + 1..].trim_start().starts_with('<') {
            entries.push(&header[start..index]);
            start = index + 1;
        }
    }
    entries.push(&header[start..]);
    let parameter = |entry: &str, name: &str| {
        let marker = format!("{name}=\"");
        entry
            .to_ascii_lowercase()
            .find(&marker)
            .and_then(|index| entry[index + marker.len()..].split_once('"'))
            .map(|(value, _)| value.to_string())
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let lower = entry.to_ascii_lowercase();
            if !lower.contains("rel=\"ice-server\"") && !lower.contains("rel=ice-server") {
                return None;
            }
            let url = entry.split_once('<')?.1.split_once('>')?.0.to_string();
            Some((
                url,
                parameter(entry, "username"),
                parameter(entry, "credential"),
            ))
        })
        .collect()
}
fn ice_servers(header: &str) -> js_sys::Array {
    let result = js_sys::Array::new();
    for (url, username, credential) in parse_ice_servers(header) {
        let server = RtcIceServer::new();
        server.set_urls(&js_sys::Array::of1(&url.into()));
        if let Some(value) = username {
            server.set_username(&value)
        }
        if let Some(value) = credential {
            server.set_credential(&value)
        }
        result.push(&server);
    }
    result
}

async fn fetch_media(
    url: &str,
    method: &str,
    token: &str,
    body: Option<&str>,
    signal: Option<&web_sys::AbortSignal>,
) -> Result<Response, JsValue> {
    let init = RequestInit::new();
    init.set_method(method);
    init.set_redirect(RequestRedirect::Error);
    init.set_signal(signal);
    let headers = web_sys::Headers::new()?;
    headers.set("Authorization", &format!("Bearer {token}"))?;
    if let Some(body) = body {
        headers.set("Content-Type", "application/sdp")?;
        init.set_body(&body.into());
    }
    init.set_headers(&headers);
    JsFuture::from(
        web_sys::window()
            .unwrap()
            .fetch_with_str_and_init(url, &init),
    )
    .await?
    .dyn_into()
}
type TrackHandler = Closure<dyn FnMut(web_sys::RtcTrackEvent)>;
type ConnectionHandler = Closure<dyn FnMut(web_sys::Event)>;
struct Whep {
    video: HtmlVideoElement,
    peer: RefCell<Option<RtcPeerConnection>>,
    resource: RefCell<Option<String>>,
    token: String,
    closed: Cell<bool>,
    controller: web_sys::AbortController,
    track: RefCell<Option<TrackHandler>>,
    state: RefCell<Option<ConnectionHandler>>,
}
impl Whep {
    fn new(video: HtmlVideoElement, token: String) -> Rc<Self> {
        Rc::new(Self {
            video,
            peer: RefCell::new(None),
            resource: RefCell::new(None),
            token,
            closed: Cell::new(false),
            controller: web_sys::AbortController::new().unwrap(),
            track: RefCell::new(None),
            state: RefCell::new(None),
        })
    }
    fn close(&self) {
        self.closed.set(true);
        self.controller.abort();
        if let Some(resource) = self.resource.borrow_mut().take() {
            let token = self.token.clone();
            spawn_local(async move {
                let init = RequestInit::new();
                init.set_method("DELETE");
                init.set_redirect(RequestRedirect::Error);
                let _ = js_sys::Reflect::set(init.as_ref(), &"keepalive".into(), &true.into());
                if let Ok(headers) = web_sys::Headers::new() {
                    let _ = headers.set("Authorization", &format!("Bearer {token}"));
                    init.set_headers(&headers);
                    let _ = JsFuture::from(
                        web_sys::window()
                            .unwrap()
                            .fetch_with_str_and_init(&resource, &init),
                    )
                    .await;
                }
            });
        }
        if let Some(peer) = self.peer.borrow_mut().take() {
            peer.set_ontrack(None);
            peer.set_onconnectionstatechange(None);
            peer.close();
        }
        self.video.set_src_object(None);
        self.track.borrow_mut().take();
        self.state.borrow_mut().take();
    }
    fn active(&self) -> Result<(), JsValue> {
        if self.closed.get() {
            Err(JsValue::from_str("Player closed"))
        } else {
            Ok(())
        }
    }
    async fn start(self: &Rc<Self>, url: &str, lost: Rc<dyn Fn()>) -> Result<(), JsValue> {
        let base = web_sys::window().unwrap().location().href()?;
        let endpoint = media_url(url, &base)?;
        let servers = match fetch_media(
            &endpoint,
            "OPTIONS",
            &self.token,
            None,
            Some(&self.controller.signal()),
        )
        .await
        {
            Ok(response) => ice_servers(&response.headers().get("Link")?.unwrap_or_default()),
            Err(_) => js_sys::Array::new(),
        };
        self.active()?;
        let config = RtcConfiguration::new();
        config.set_ice_servers(&servers);
        let peer = RtcPeerConnection::new_with_configuration(&config)?;
        *self.peer.borrow_mut() = Some(peer.clone());
        let init = RtcRtpTransceiverInit::new();
        init.set_direction(RtcRtpTransceiverDirection::Recvonly);
        peer.add_transceiver_with_str_and_init("video", &init);
        peer.add_transceiver_with_str_and_init("audio", &init);
        let this = self.clone();
        let track = Closure::wrap(Box::new(move |event: web_sys::RtcTrackEvent| {
            if !this.closed.get()
                && let Ok(stream) = event.streams().get(0).dyn_into::<web_sys::MediaStream>()
            {
                this.video.set_src_object(Some(&stream));
            }
        }) as Box<dyn FnMut(_)>);
        peer.set_ontrack(Some(track.as_ref().unchecked_ref()));
        *self.track.borrow_mut() = Some(track);
        let offer = JsFuture::from(peer.create_offer()).await?;
        self.active()?;
        let offer: RtcSessionDescriptionInit = offer.unchecked_into();
        JsFuture::from(peer.set_local_description(&offer)).await?;
        // Polling is bounded and checks cancellation even on browsers that omit
        // a final ICE event. The original five-second gathering budget remains.
        for _ in 0..50 {
            self.active()?;
            if peer.ice_gathering_state() == web_sys::RtcIceGatheringState::Complete {
                break;
            }
            gloo_timers::future::TimeoutFuture::new(100).await;
        }
        let local = peer
            .local_description()
            .ok_or_else(|| JsValue::from_str("Missing WHEP SDP"))?;
        let response = fetch_media(
            &endpoint,
            "POST",
            &self.token,
            Some(&local.sdp()),
            Some(&self.controller.signal()),
        )
        .await?;
        if !response.ok() {
            return Err(JsValue::from_str("WHEP signaling failed"));
        }
        if let Some(location) = response.headers().get("Location")?
            && !location.is_empty()
        {
            *self.resource.borrow_mut() = Some(resource_url(&location, &endpoint, &base)?);
        }
        self.active()?;
        let answer = JsFuture::from(response.text()?)
            .await?
            .as_string()
            .unwrap_or_default();
        let desc = RtcSessionDescriptionInit::new(RtcSdpType::Answer);
        desc.set_sdp(&answer);
        JsFuture::from(peer.set_remote_description(&desc)).await?;
        let mut connected = false;
        for _ in 0..120 {
            self.active()?;
            match peer.connection_state() {
                web_sys::RtcPeerConnectionState::Connected => {
                    connected = true;
                    break;
                }
                web_sys::RtcPeerConnectionState::Failed
                | web_sys::RtcPeerConnectionState::Closed => {
                    return Err(JsValue::from_str("WebRTC connection failed"));
                }
                _ => (),
            }
            gloo_timers::future::TimeoutFuture::new(100).await;
        }
        if !connected {
            return Err(JsValue::from_str("WebRTC connection timed out"));
        }
        self.active()?;
        let this = self.clone();
        let state = Closure::wrap(Box::new(move |_: web_sys::Event| {
            if !this.closed.get()
                && this.peer.borrow().as_ref().is_some_and(|p| {
                    p.connection_state() == web_sys::RtcPeerConnectionState::Failed
                })
            {
                lost();
            }
        }) as Box<dyn FnMut(_)>);
        peer.set_onconnectionstatechange(Some(state.as_ref().unchecked_ref()));
        *self.state.borrow_mut() = Some(state);
        if let Ok(promise) = self.video.play() {
            let _ = JsFuture::from(promise).await;
        }
        Ok(())
    }
}
enum Active {
    Whep(Rc<Whep>),
    Hls {
        engine: JsValue,
        _manifest: Closure<dyn FnMut()>,
        _failure: Closure<dyn FnMut()>,
    },
}
impl Active {
    fn close(self) {
        match self {
            Self::Whep(w) => w.close(),
            Self::Hls { engine, .. } => destroy_hls(&engine),
        }
    }
}
struct Player {
    closed: Cell<bool>,
    generation: Cell<u64>,
    active: RefCell<Option<Active>>,
    timer: RefCell<Option<gloo_timers::callback::Timeout>>,
    video: HtmlVideoElement,
    camera: String,
    profile: String,
    label: RwSignal<String>,
}
impl Player {
    fn stop_active(&self) {
        if let Some(active) = self.active.borrow_mut().take() {
            active.close()
        }
        self.video.pause().ok();
        self.video.set_src_object(None);
        self.video.remove_attribute("src").ok();
        self.video.load();
    }
    fn close(&self) {
        self.closed.set(true);
        self.generation.set(self.generation.get() + 1);
        self.timer.borrow_mut().take();
        self.stop_active();
    }
    fn current(&self, g: u64) -> bool {
        !self.closed.get() && self.generation.get() == g
    }
    fn schedule(self: &Rc<Self>, attempt: usize) {
        if self.closed.get() {
            return;
        }
        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.timer.borrow_mut().take();
        self.stop_active();
        if attempt >= 3 {
            self.label
                .set(t("视频暂不可用", "Video temporarily unavailable"));
            let this = self.clone();
            *self.timer.borrow_mut() =
                Some(gloo_timers::callback::Timeout::new(15_000, move || {
                    this.schedule(0)
                }));
            return;
        }
        self.label.set(if attempt == 0 {
            t("正在连接", "Connecting")
        } else {
            t("正在重新连接", "Reconnecting")
        });
        let this = self.clone();
        *self.timer.borrow_mut() = Some(gloo_timers::callback::Timeout::new(
            [0, 1_000, 3_000][attempt],
            move || {
                spawn_local(async move {
                    this.connect(attempt, generation).await;
                });
            },
        ));
    }
    async fn ticket(&self) -> Result<Ticket, xcss_web::api::ApiError> {
        request(
            &format!(
                "/cameras/{}/stream-ticket?profile={}",
                self.camera, self.profile
            ),
            "GET",
            None,
        )
        .await
    }
    async fn connect(self: Rc<Self>, attempt: usize, g: u64) {
        let result = async {
            let ticket = self
                .ticket()
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if !self.current(g) {
                return Ok(());
            }
            let whep = Whep::new(self.video.clone(), ticket.token);
            *self.active.borrow_mut() = Some(Active::Whep(whep.clone()));
            let this = self.clone();
            if whep
                .start(
                    &ticket.whep_url,
                    Rc::new(move || {
                        if this.current(g) {
                            this.schedule(attempt + 1)
                        }
                    }),
                )
                .await
                .is_ok()
            {
                if self.current(g) {
                    self.label.set(String::new())
                }
                return Ok(());
            }
            whep.close();
            if !self.current(g) {
                return Ok(());
            }
            self.active.borrow_mut().take();
            // A failed WHEP attempt may consume its ticket. Always obtain a fresh
            // HLS ticket rather than reusing a stale credential.
            let fallback = self
                .ticket()
                .await
                .map_err(|e| JsValue::from_str(&e.to_string()))?;
            if !self.current(g) {
                return Ok(());
            }
            let this = self.clone();
            let manifest = Closure::wrap(Box::new(move || {
                if this.current(g) {
                    this.label.set(String::new());
                    if let Ok(play) = this.video.play() {
                        let this = this.clone();
                        spawn_local(async move {
                            if JsFuture::from(play).await.is_err() && this.current(g) {
                                this.label.set(t("请点击播放", "Click to play"));
                            }
                        });
                    }
                }
            }) as Box<dyn FnMut()>);
            let this = self.clone();
            let error = Closure::wrap(Box::new(move || {
                if this.current(g) {
                    this.schedule(attempt + 1)
                }
            }) as Box<dyn FnMut()>);
            let promise = create_hls(
                &self.video,
                &fallback.hls_url,
                &fallback.token,
                manifest.as_ref().unchecked_ref(),
                error.as_ref().unchecked_ref(),
            )?;
            let hls = JsFuture::from(promise).await?;
            if !self.current(g) {
                destroy_hls(&hls);
                return Ok(());
            }
            *self.active.borrow_mut() = Some(Active::Hls {
                engine: hls,
                _manifest: manifest,
                _failure: error,
            });
            Ok::<(), JsValue>(())
        }
        .await;
        if result.is_err() && self.current(g) {
            self.schedule(attempt + 1)
        }
    }
}

#[component]
pub fn LiveVideo(
    camera: Camera,
    #[prop(default="main".into())] profile: String,
    #[prop(default = false)] controls: bool,
) -> impl IntoView {
    let video = NodeRef::<html::Video>::new();
    let label = RwSignal::new(if camera.enabled {
        t("正在连接", "Connecting")
    } else {
        t("设备已停用", "Device disabled")
    });
    let player = StoredValue::new_local(None::<Rc<Player>>);
    Effect::new(move |_| {
        if let Some(video) = video.get()
            && camera.enabled
        {
            let next = Rc::new(Player {
                closed: Cell::new(false),
                generation: Cell::new(0),
                active: RefCell::new(None),
                timer: RefCell::new(None),
                video: video.unchecked_into(),
                camera: camera.id.clone(),
                profile: profile.clone(),
                label,
            });
            next.schedule(0);
            player.set_value(Some(next));
        }
    });
    on_cleanup(move || {
        player.update_value(|p| {
            if let Some(p) = p.take() {
                p.close()
            }
        })
    });
    View::new(
        xcss_web::html::div()
            .child((
                xcss_web::html::video()
                    .node_ref(video)
                    .muted(true)
                    .autoplay(true)
                    .playsinline(true)
                    .controls(
                        ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(controls),
                    ),
                {
                    #[allow(unreachable_code)]
                    #[allow(unused_mut)]
                    #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                    component_view(
                        #[allow(clippy::needless_borrows_for_generic_args)]
                        &Show,
                        {
                            let mut props = component_props_builder(&Show)
                                .when(
                                    #[allow(unused_braces)]
                                    {
                                        move || !label.get().is_empty()
                                    },
                                )
                                .children({
                                    ::xcss_web::children::ToChildren::to_children(move || {
                                        xcss_web::html::div()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({ move || label.get() })
                                                },
                                            )
                                            .class("video-state")
                                    })
                                })
                                .build();
                            props
                        },
                    )
                },
                {
                    #[allow(unreachable_code)]
                    #[allow(unused_mut)]
                    #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                    component_view(
                        #[allow(clippy::needless_borrows_for_generic_args)]
                        &Show,
                        {
                            let mut props = component_props_builder(&Show)
                                .when(
                                    #[allow(unused_braces)]
                                    {
                                        move || !controls
                                    },
                                )
                                .children({
                                    ::xcss_web::children::ToChildren::to_children(move || {
                                        xcss_web::html::div().class("scanline")
                                    })
                                })
                                .build();
                            props
                        },
                    )
                },
            ))
            .class(if controls {
                "detail-video"
            } else {
                "video-shell"
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ice_link_headers_keep_quoted_commas_and_multiple_servers() {
        let servers = parse_ice_servers(
            r#"<stun:lan.example>; rel="ice-server",  <turn:relay.example?transport=udp>; REL="ice-server"; username="a,b"; credential="Secret", <https://other.example>; rel="alternate""#,
        );
        assert_eq!(
            servers,
            vec![
                ("stun:lan.example".into(), None, None),
                (
                    "turn:relay.example?transport=udp".into(),
                    Some("a,b".into()),
                    Some("Secret".into())
                )
            ]
        );
    }
}
