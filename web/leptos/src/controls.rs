use crate::{AppState, CameraStatus, Failure, Observation, media::LiveVideo, model::*};
use leptos::prelude::*;
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use xcss_web::i18n::t;

struct Ptz {
    id: String,
    moving: Cell<bool>,
    busy: Cell<bool>,
    inflight: Cell<bool>,
    stop_requested: Cell<bool>,
    closed: Cell<bool>,
    repeat: RefCell<Option<gloo_timers::callback::Interval>>,
    receipt: RwSignal<Option<Receipt>>,
    error: RwSignal<Option<xcss_web::api::ApiError>>,
}
impl Ptz {
    fn start(self: &Rc<Self>, vector: [f64; 3]) {
        if self.busy.replace(true) {
            return;
        }
        self.moving.set(true);
        self.move_now(vector);
        let this = self.clone();
        *self.repeat.borrow_mut() = Some(gloo_timers::callback::Interval::new(2_000, move || {
            this.move_now(vector)
        }));
    }
    fn move_now(self: &Rc<Self>, vector: [f64; 3]) {
        if !self.moving.get() || self.inflight.replace(true) {
            return;
        }
        self.send("move", vector);
    }
    fn stop(self: &Rc<Self>) {
        if !self.moving.replace(false) {
            return;
        }
        self.repeat.borrow_mut().take();
        if self.inflight.get() {
            self.stop_requested.set(true)
        } else {
            self.inflight.set(true);
            self.send("stop", [0., 0., 0.]);
        }
    }
    fn explicit_stop(self: &Rc<Self>) {
        if self.moving.get() {
            self.stop()
        } else if !self.busy.replace(true) {
            self.inflight.set(true);
            self.send("stop", [0., 0., 0.]);
        }
    }
    fn send(self: &Rc<Self>, action: &'static str, vector: [f64; 3]) {
        let this = self.clone();
        spawn_local(async move {
            let result = request::<Receipt>(
                &format!("/cameras/{}/ptz", this.id),
                "POST",
                Some(json!({"action":action,"pan":vector[0],"tilt":vector[1],"zoom":vector[2]})),
            )
            .await;
            if !this.closed.get() {
                match result {
                    Ok(receipt) => this.receipt.set(Some(receipt)),
                    Err(error) => this.error.set(Some(error)),
                }
            }
            this.inflight.set(false);
            if this.stop_requested.replace(false) {
                this.inflight.set(true);
                this.send("stop", [0., 0., 0.]);
            } else if action == "stop" {
                this.busy.set(false);
            }
        });
    }
}

#[component]
pub fn CameraDrawer(id: String, close: Callback<()>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    Effect::new(move |_| {
        if let Some(dialog) = dialog.get() {
            let _ = dialog.show_modal();
        }
    });
    let cid = id.clone();
    let camera = Signal::derive(move || app.cameras.get().into_iter().find(|c| c.id == cid));
    let receipt = RwSignal::new(None::<Receipt>);
    let error = RwSignal::new(None);
    let version = RwSignal::new(0u64);
    let ptz = StoredValue::new_local(Rc::new(Ptz {
        id: id.clone(),
        moving: Cell::new(false),
        busy: Cell::new(false),
        inflight: Cell::new(false),
        stop_requested: Cell::new(false),
        closed: Cell::new(false),
        repeat: RefCell::new(None),
        receipt,
        error,
    }));
    let listeners = StoredValue::new_local(vec![
        gloo_events::EventListener::new(&web_sys::window().unwrap(), "blur", move |_| {
            ptz.try_with_value(|p| p.stop());
        }),
        gloo_events::EventListener::new(
            &web_sys::window().unwrap().document().unwrap(),
            "visibilitychange",
            move |_| {
                if web_sys::window().unwrap().document().unwrap().hidden() {
                    ptz.try_with_value(|p| p.stop());
                }
            },
        ),
    ]);
    let poll = StoredValue::new_local(None::<gloo_timers::callback::Timeout>);
    let controller = StoredValue::new_local(None::<web_sys::AbortController>);
    let alive = RwSignal::new(true);
    let poll_id = id.clone();
    Effect::new(move |_| {
        let current = receipt.get();
        version.track();
        poll.update_value(|v| {
            v.take();
        });
        controller.update_value(|v| {
            if let Some(c) = v.take() {
                c.abort()
            }
        });
        if let Some(current) = current
            && ["queued", "awaiting_result"].contains(&current.state.as_str())
        {
            error.set(None);
            let id = poll_id.clone();
            let command = current.command_id;
            poll.set_value(Some(gloo_timers::callback::Timeout::new(
                1_000,
                move || {
                    let abort = web_sys::AbortController::new().unwrap();
                    controller.set_value(Some(abort.clone()));
                    spawn_local(async move {
                        let result = xcss_web::api::request_with_options(
                            &format!("/api/v1/cameras/{id}/commands/{command}"),
                            "GET",
                            None,
                            xcss_web::api::RequestOptions {
                                signal: Some(abort.signal()),
                                ..Default::default()
                            },
                        )
                        .await
                        .and_then(|v| {
                            serde_json::from_value::<Receipt>(v).map_err(|_| contract_error())
                        });
                        if alive.try_get_untracked() != Some(true) || abort.signal().aborted() {
                            return;
                        }
                        match result {
                            Ok(value) => receipt.set(Some(value)),
                            Err(e) => error.set(Some(e)),
                        }
                    });
                },
            )));
        }
    });
    let cleanup_ptz = send_wrapper::SendWrapper::new(ptz.with_value(Clone::clone));
    on_cleanup(move || {
        alive.set(false);
        cleanup_ptz.stop();
        cleanup_ptz.closed.set(true);
        listeners.dispose();
        poll.dispose();
        controller.update_value(|v| {
            if let Some(c) = v.take() {
                c.abort()
            }
        });
    });
    let movement = move |vector: [f64; 3],
                         zh: &'static str,
                         en: &'static str,
                         glyph: &'static str| {
        view! {<button class="xcss-button" aria-label=t(zh,en)
               on:pointerdown=move|e:web_sys::PointerEvent|{if e.button()!=0{return}
        if let Some(target)=e.current_target().and_then(|v|v.dyn_into::<web_sys::HtmlElement>().ok()){let _=target.set_pointer_capture(e.pointer_id());}ptz.try_with_value(|p|p.start(vector));}
               on:pointerup=move |_|{ptz.try_with_value(|p|p.stop());} on:pointercancel=move |_|{ptz.try_with_value(|p|p.stop());} on:lostpointercapture=move |_|{ptz.try_with_value(|p|p.stop());} on:blur=move |_|{ptz.try_with_value(|p|p.stop());}
               on:keydown=move|e:web_sys::KeyboardEvent|{if e.key()==" "||e.key()=="Enter"{e.prevent_default();if !e.repeat(){ptz.try_with_value(|p|p.start(vector));}}}
               on:keyup=move|e:web_sys::KeyboardEvent|{if e.key()==" "||e.key()=="Enter"{e.prevent_default();ptz.try_with_value(|p|p.stop());}}>{match glyph{"−"=>t("− 拉远","− Zoom out"),"＋"=>t("＋ 拉近","+ Zoom in"),_=>glyph.to_string()}}</button>}
    };
    view! {<dialog node_ref=dialog on:cancel=move|event:web_sys::Event|{event.prevent_default();ptz.try_with_value(|p|p.stop());close.try_run(());} class="xcss-dialog xcos-camera-dialog" aria-modal="true" aria-label=move||camera.get().map(|c|c.name).unwrap_or_default()><header class="xcss-dialog-heading"><h2>{move||camera.get().map(|c|c.name)}</h2><button class="xcss-button" aria-label=t("关闭","Close") on:click=move |_|{ptz.try_with_value(|p|p.stop());close.try_run(());}>"×"</button></header><p>{move||camera.get().map(|c|if c.location.is_empty(){t("未标注位置","Location not specified")}else{c.location})}</p><div class="xcos-business">
        <For each=move||camera.get().map(|c|vec![(c.id,c.enabled)]).unwrap_or_default() key=|v|v.clone() children=move|_|view!{<LiveVideo camera=camera.get_untracked().unwrap() controls=true/>}/>
        <section class="xcss-content-panel"><h3>{t("设备信息","Device information")}</h3><CameraStatus camera=camera/><Observation camera=camera/><p>{move||camera.get().map(|c|{let text=[c.manufacturer,c.model,c.firmware_version].into_iter().flatten().collect::<Vec<_>>().join(" · ");if text.is_empty(){t("设备未报告厂商信息","The device did not report make information")}else{text}})}</p><p>{t("适配器","Adapter")}": "{move||camera.get().map(|c|c.adapter_kind.to_uppercase())}" · "{t("能力","Capabilities")}": "{move||camera.get().map(|c|{[("video",t("视频","Video"),c.capabilities.video),("sub",t("子码流","Sub-stream"),c.capabilities.sub_stream),("ptz","PTZ".into(),c.capabilities.ptz),("events",t("事件","Events"),c.capabilities.events),("audio",t("音频","Audio"),c.capabilities.audio_input)].into_iter().map(|(_,label,state)|match state.as_str(){"supported"=>label,"unknown"=>format!("{}: {}",label,t("未确认","unconfirmed")),_=>format!("{}: {}",label,t("不支持","unsupported"))}).collect::<Vec<_>>().join(" / ")})}</p><Show when=move||camera.get().is_some_and(|c|c.health_message.is_some())><p>{t("健康状态","Health")}": "{move||camera.get().and_then(|c|c.health_message)}</p></Show></section>
        <Show when=move||camera.get().is_some_and(|c|c.capabilities.ptz=="supported")><section class="ptz-panel" aria-label=t("云台控制","PTZ controls")><h3>{t("云台控制","PTZ controls")}</h3><p>{t("按住方向键或用空格、回车启动移动，松开即停止。窗口失焦也会发送停止。","Hold a direction button, Space or Enter to move; release to stop. Losing window focus also sends a stop command.")}</p><Show when=move||receipt.get().is_some()&&error.get().is_none()><p role="status">{move||receipt.get().map(|r|match r.state.as_str(){"queued"=>t("已排队，尚未发送到设备","Queued; not yet sent to the device"),"awaiting_result"=>t("已发送，等待设备回执","Sent; waiting for the device result"),"succeeded"=>t("设备已确认执行","The device confirmed execution"),"failed"=>t("设备命令执行失败","Device command failed"),"expired"=>t("命令在发送前已过期","Command expired before delivery"),_=>t("设备执行结果未确认","Device execution remains unconfirmed")})}</p></Show><Failure error=error/><Show when=move||error.get().is_some()><button class="xcss-button" on:click=move |_|version.update(|v|*v+=1)>{t("重试状态查询","Retry status lookup")}</button></Show>
        <div class="ptz-grid"><span/>{movement([0.,0.55,0.],"云台向上","Tilt up","↑")}<span/>{movement([-0.55,0.,0.],"云台向左","Pan left","←")}<button class="xcss-button" aria-label=t("停止云台","Stop movement") on:click=move |_|{ptz.try_with_value(|p|p.explicit_stop());}>"■"</button>{movement([0.55,0.,0.],"云台向右","Pan right","→")}<span/>{movement([0.,-0.55,0.],"云台向下","Tilt down","↓")}<span/></div><div class="zoom-row">{movement([0.,0.,-0.45],"云台拉远","Zoom out","−")}{movement([0.,0.,0.45],"云台拉近","Zoom in","＋")}</div>
        </section></Show>
    </div></dialog>}
}
