use crate::{AppState, CameraStatus, Failure, Observation, media::LiveVideo, model::*};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::spawn_local;
use xcss_web::component::{component_props_builder, component_view};
use xcss_web::i18n::t;
use xcss_web::prelude::*;

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
    let dialog = NodeRef::<xcss_web::html::Dialog>::new();
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
    let movement =
        move |vector: [f64; 3], zh: &'static str, en: &'static str, glyph: &'static str| {
            View::new(
                xcss_web::html::button()
                    .child(
                        #[allow(unused_braces)]
                        {
                            IntoRender::into_render({
                                match glyph {
                                    "−" => t("− 拉远", "− Zoom out"),
                                    "＋" => t("＋ 拉近", "+ Zoom in"),
                                    _ => glyph.to_string(),
                                }
                            })
                        },
                    )
                    .class("xcss-button")
                    .aria_label(
                        ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(t(zh, en)),
                    )
                    .on(
                        xcss_web::ev::pointerdown,
                        move |e: web_sys::PointerEvent| {
                            if e.button() != 0 {
                                return;
                            }
                            if let Some(target) = e
                                .current_target()
                                .and_then(|v| v.dyn_into::<web_sys::HtmlElement>().ok())
                            {
                                let _ = target.set_pointer_capture(e.pointer_id());
                            }
                            ptz.try_with_value(|p| p.start(vector));
                        },
                    )
                    .on(xcss_web::ev::pointerup, move |_| {
                        ptz.try_with_value(|p| p.stop());
                    })
                    .on(xcss_web::ev::pointercancel, move |_| {
                        ptz.try_with_value(|p| p.stop());
                    })
                    .on(xcss_web::ev::lostpointercapture, move |_| {
                        ptz.try_with_value(|p| p.stop());
                    })
                    .on(xcss_web::ev::blur, move |_| {
                        ptz.try_with_value(|p| p.stop());
                    })
                    .on(xcss_web::ev::keydown, move |e: web_sys::KeyboardEvent| {
                        if e.key() == " " || e.key() == "Enter" {
                            e.prevent_default();
                            if !e.repeat() {
                                ptz.try_with_value(|p| p.start(vector));
                            }
                        }
                    })
                    .on(xcss_web::ev::keyup, move |e: web_sys::KeyboardEvent| {
                        if e.key() == " " || e.key() == "Enter" {
                            e.prevent_default();
                            ptz.try_with_value(|p| p.stop());
                        }
                    }),
            )
        };
    View::new(
    xcss_web::html::dialog()
        .child((
            xcss_web::html::header()
                .child((
                    xcss_web::html::h2()
                        .child(
                            #[allow(unused_braces)]
                            {
                                IntoRender::into_render({
                                    move || camera.get().map(|c| c.name)
                                })
                            },
                        ),
                    xcss_web::html::button()
                        .child(#[allow(unused_braces)] { "×" })
                        .class("xcss-button")
                        .aria_label(
                            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                t("关闭", "Close"),
                            ),
                        )
                        .on(
                            xcss_web::ev::click,
                            move |_| {
                                ptz.try_with_value(|p| p.stop());
                                close.try_run(());
                            },
                        ),
                ))
                .class("xcss-dialog-heading"),
            xcss_web::html::p()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || {
                                camera
                                    .get()
                                    .map(|c| {
                                        if c.location.is_empty() {
                                            t("未标注位置", "Location not specified")
                                        } else {
                                            c.location
                                        }
                                    })
                            }
                        })
                    },
                ),
            xcss_web::html::div()
                .child((
                    {
                        #[allow(unreachable_code)] #[allow(unused_mut)]
                        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                        component_view(
                            #[allow(clippy::needless_borrows_for_generic_args)]
                            &For,
                            {
                                let mut props = component_props_builder(
                                        &For,
                                    )
                                    .each(
                                        #[allow(unused_braces)]
                                        {
                                            move || {
                                                camera
                                                    .get()
                                                    .map(|c| vec![(c.id, c.enabled)])
                                                    .unwrap_or_default()
                                            }
                                        },
                                    )
                                    .key(#[allow(unused_braces)] { |v| v.clone() })
                                    .children(
                                        #[allow(unused_braces)]
                                        {
                                            move |_| {
                                                View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &LiveVideo,
        {
            let mut props = component_props_builder(&LiveVideo)
                .camera(#[allow(unused_braces)] { camera.get_untracked().unwrap() })
                .controls(#[allow(unused_braces)] { true })
                .build();
            props
        },
    )
})
                                            }
                                        },
                                    )
                                    .build();
                                props
                            },
                        )
                    },
                    xcss_web::html::section()
                        .child((
                            xcss_web::html::h3()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("设备信息", "Device information")
                                        })
                                    },
                                ),
                            {
                                #[allow(unreachable_code)] #[allow(unused_mut)]
                                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &CameraStatus,
                                    {
                                        let mut props = component_props_builder(
                                                &CameraStatus,
                                            )
                                            .camera(#[allow(unused_braces)] { camera })
                                            .build();
                                        props
                                    },
                                )
                            },
                            {
                                #[allow(unreachable_code)] #[allow(unused_mut)]
                                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &Observation,
                                    {
                                        let mut props = component_props_builder(
                                                &Observation,
                                            )
                                            .camera(#[allow(unused_braces)] { camera })
                                            .build();
                                        props
                                    },
                                )
                            },
                            xcss_web::html::p()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || {
                                                camera
                                                    .get()
                                                    .map(|c| {
                                                        let text = [c.manufacturer, c.model, c.firmware_version]
                                                            .into_iter()
                                                            .flatten()
                                                            .collect::<Vec<_>>()
                                                            .join(" · ");
                                                        if text.is_empty() {
                                                            t(
                                                                "设备未报告厂商信息",
                                                                "The device did not report make information",
                                                            )
                                                        } else {
                                                            text
                                                        }
                                                    })
                                            }
                                        })
                                    },
                                ),
                            xcss_web::html::p()
                                .child((
                                    IntoRender::into_render({
                                        t("适配器", "Adapter")
                                    }),
                                    ": ",
                                    IntoRender::into_render({
                                        move || camera.get().map(|c| c.adapter_kind.to_uppercase())
                                    }),
                                    " · ",
                                    IntoRender::into_render({
                                        t("能力", "Capabilities")
                                    }),
                                    ": ",
                                    IntoRender::into_render({
                                        move || {
                                            camera
                                                .get()
                                                .map(|c| {
                                                    [
                                                        ("video", t("视频", "Video"), c.capabilities.video),
                                                        (
                                                            "sub",
                                                            t("子码流", "Sub-stream"),
                                                            c.capabilities.sub_stream,
                                                        ),
                                                        ("ptz", "PTZ".into(), c.capabilities.ptz),
                                                        ("events", t("事件", "Events"), c.capabilities.events),
                                                        ("audio", t("音频", "Audio"), c.capabilities.audio_input),
                                                    ]
                                                        .into_iter()
                                                        .map(|(_, label, state)| match state.as_str() {
                                                            "supported" => label,
                                                            "unknown" => {
                                                                format!("{}: {}", label, t("未确认", "unconfirmed"))
                                                            }
                                                            _ => format!("{}: {}", label, t("不支持", "unsupported")),
                                                        })
                                                        .collect::<Vec<_>>()
                                                        .join(" / ")
                                                })
                                        }
                                    }),
                                )),
                            {
                                #[allow(unreachable_code)] #[allow(unused_mut)]
                                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &Show,
                                    {
                                        let mut props = component_props_builder(
                                                &Show,
                                            )
                                            .when(
                                                #[allow(unused_braces)]
                                                {
                                                    move || {
                                                        camera.get().is_some_and(|c| c.health_message.is_some())
                                                    }
                                                },
                                            )
                                            .children({
                                                ::xcss_web::children::ToChildren::to_children(move || {
                                                    xcss_web::html::p()
                                                        .child((
                                                            IntoRender::into_render({
                                                                t("健康状态", "Health")
                                                            }),
                                                            ": ",
                                                            IntoRender::into_render({
                                                                move || camera.get().and_then(|c| c.health_message)
                                                            }),
                                                        ))
                                                })
                                            })
                                            .build();
                                        props
                                    },
                                )
                            },
                        ))
                        .class("xcss-content-panel"),
                    {
                        #[allow(unreachable_code)] #[allow(unused_mut)]
                        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                        component_view(
                            #[allow(clippy::needless_borrows_for_generic_args)]
                            &Show,
                            {
                                let mut props = component_props_builder(
                                        &Show,
                                    )
                                    .when(
                                        #[allow(unused_braces)]
                                        {
                                            move || {
                                                camera
                                                    .get()
                                                    .is_some_and(|c| c.capabilities.ptz == "supported")
                                            }
                                        },
                                    )
                                    .children({
                                        ::xcss_web::children::ToChildren::to_children(move || {
                                            xcss_web::html::section()
                                                .child((
                                                    xcss_web::html::h3()
                                                        .child(
                                                            #[allow(unused_braces)]
                                                            {
                                                                IntoRender::into_render({
                                                                    t("云台控制", "PTZ controls")
                                                                })
                                                            },
                                                        ),
                                                    xcss_web::html::p()
                                                        .child(
                                                            #[allow(unused_braces)]
                                                            {
                                                                IntoRender::into_render({
                                                                    t(
                                                                        "按住方向键或用空格、回车启动移动，松开即停止。窗口失焦也会发送停止。",
                                                                        "Hold a direction button, Space or Enter to move; release to stop. Losing window focus also sends a stop command.",
                                                                    )
                                                                })
                                                            },
                                                        ),
                                                    {
                                                        #[allow(unreachable_code)] #[allow(unused_mut)]
                                                        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                                        component_view(
                                                            #[allow(clippy::needless_borrows_for_generic_args)]
                                                            &Show,
                                                            {
                                                                let mut props = component_props_builder(
                                                                        &Show,
                                                                    )
                                                                    .when(
                                                                        #[allow(unused_braces)]
                                                                        { move || receipt.get().is_some() && error.get().is_none() },
                                                                    )
                                                                    .children({
                                                                        ::xcss_web::children::ToChildren::to_children(move || {
                                                                            xcss_web::html::p()
                                                                                .child(
                                                                                    #[allow(unused_braces)]
                                                                                    {
                                                                                        IntoRender::into_render({
                                                                                            move || {
                                                                                                receipt
                                                                                                    .get()
                                                                                                    .map(|r| match r.state.as_str() {
                                                                                                        "queued" => {
                                                                                                            t(
                                                                                                                "已排队，尚未发送到设备",
                                                                                                                "Queued; not yet sent to the device",
                                                                                                            )
                                                                                                        }
                                                                                                        "awaiting_result" => {
                                                                                                            t(
                                                                                                                "已发送，等待设备回执",
                                                                                                                "Sent; waiting for the device result",
                                                                                                            )
                                                                                                        }
                                                                                                        "succeeded" => {
                                                                                                            t("设备已确认执行", "The device confirmed execution")
                                                                                                        }
                                                                                                        "failed" => {
                                                                                                            t("设备命令执行失败", "Device command failed")
                                                                                                        }
                                                                                                        "expired" => {
                                                                                                            t(
                                                                                                                "命令在发送前已过期",
                                                                                                                "Command expired before delivery",
                                                                                                            )
                                                                                                        }
                                                                                                        _ => {
                                                                                                            t(
                                                                                                                "设备执行结果未确认",
                                                                                                                "Device execution remains unconfirmed",
                                                                                                            )
                                                                                                        }
                                                                                                    })
                                                                                            }
                                                                                        })
                                                                                    },
                                                                                )
                                                                                .role("status")
                                                                        })
                                                                    })
                                                                    .build();
                                                                props
                                                            },
                                                        )
                                                    },
                                                    {
                                                        #[allow(unreachable_code)] #[allow(unused_mut)]
                                                        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                                        component_view(
                                                            #[allow(clippy::needless_borrows_for_generic_args)]
                                                            &Failure,
                                                            {
                                                                let mut props = component_props_builder(
                                                                        &Failure,
                                                                    )
                                                                    .error(#[allow(unused_braces)] { error })
                                                                    .build();
                                                                props
                                                            },
                                                        )
                                                    },
                                                    {
                                                        #[allow(unreachable_code)] #[allow(unused_mut)]
                                                        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                                        component_view(
                                                            #[allow(clippy::needless_borrows_for_generic_args)]
                                                            &Show,
                                                            {
                                                                let mut props = component_props_builder(
                                                                        &Show,
                                                                    )
                                                                    .when(
                                                                        #[allow(unused_braces)]
                                                                        { move || error.get().is_some() },
                                                                    )
                                                                    .children({
                                                                        ::xcss_web::children::ToChildren::to_children(move || {
                                                                            xcss_web::html::button()
                                                                                .child(
                                                                                    #[allow(unused_braces)]
                                                                                    {
                                                                                        IntoRender::into_render({
                                                                                            t("重试状态查询", "Retry status lookup")
                                                                                        })
                                                                                    },
                                                                                )
                                                                                .class("xcss-button")
                                                                                .on(
                                                                                    xcss_web::ev::click,
                                                                                    move |_| version.update(|v| *v += 1),
                                                                                )
                                                                        })
                                                                    })
                                                                    .build();
                                                                props
                                                            },
                                                        )
                                                    },
                                                    xcss_web::html::div()
                                                        .child((
                                                            xcss_web::html::span(),
                                                            IntoRender::into_render({
                                                                movement([0., 0.55, 0.], "云台向上", "Tilt up", "↑")
                                                            }),
                                                            xcss_web::html::span(),
                                                            IntoRender::into_render({
                                                                movement([-0.55, 0., 0.], "云台向左", "Pan left", "←")
                                                            }),
                                                            xcss_web::html::button()
                                                                .child(#[allow(unused_braces)] { "■" })
                                                                .class("xcss-button")
                                                                .aria_label(
                                                                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                                                        t("停止云台", "Stop movement"),
                                                                    ),
                                                                )
                                                                .on(
                                                                    xcss_web::ev::click,
                                                                    move |_| {
                                                                        ptz.try_with_value(|p| p.explicit_stop());
                                                                    },
                                                                ),
                                                            IntoRender::into_render({
                                                                movement([0.55, 0., 0.], "云台向右", "Pan right", "→")
                                                            }),
                                                            xcss_web::html::span(),
                                                            IntoRender::into_render({
                                                                movement(
                                                                    [0., -0.55, 0.],
                                                                    "云台向下",
                                                                    "Tilt down",
                                                                    "↓",
                                                                )
                                                            }),
                                                            xcss_web::html::span(),
                                                        ))
                                                        .class("ptz-grid"),
                                                    xcss_web::html::div()
                                                        .child((
                                                            IntoRender::into_render({
                                                                movement([0., 0., -0.45], "云台拉远", "Zoom out", "−")
                                                            }),
                                                            IntoRender::into_render({
                                                                movement([0., 0., 0.45], "云台拉近", "Zoom in", "＋")
                                                            }),
                                                        ))
                                                        .class("zoom-row"),
                                                ))
                                                .class("ptz-panel")
                                                .aria_label(
                                                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                                        t("云台控制", "PTZ controls"),
                                                    ),
                                                )
                                        })
                                    })
                                    .build();
                                props
                            },
                        )
                    },
                ))
                .class("xcos-business"),
        ))
        .class("xcss-dialog xcos-camera-dialog")
        .node_ref(dialog)
        .on(
            xcss_web::ev::Custom::new("cancel"),
            move |event: web_sys::Event| {
                event.prevent_default();
                ptz.try_with_value(|p| p.stop());
                close.try_run(());
            },
        )
        .aria_modal("true")
        .aria_label(
            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move || {
                camera.get().map(|c| c.name).unwrap_or_default()
            }),
        ),
)
}
