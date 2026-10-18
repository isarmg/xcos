use xcss_web::component::{component_props_builder, component_view};
mod controls;
mod logs;
mod media;
mod model;
mod recordings;
mod resource_path;

use model::*;
use serde_json::json;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::spawn_local;
use xcss_web::prelude::*;
use xcss_web::{
    components::{AccountPage, AdminShell, HeaderActions, HeaderNavigation, notify},
    i18n::t,
};

#[derive(Clone, Copy)]
pub struct AppState {
    cameras: RwSignal<Vec<Camera>>,
    clients: RwSignal<Vec<Client>>,
    system: RwSignal<Option<System>>,
    selected: RwSignal<String>,
    clients_generation: RwSignal<u64>,
    error: RwSignal<Option<xcss_web::api::ApiError>>,
    busy: RwSignal<bool>,
    queued: RwSignal<bool>,
    logs_refresh: RwSignal<u64>,
    alive: RwSignal<bool>,
}
impl AppState {
    fn toast(self, message: String) {
        notify(message);
    }
    fn refresh(self) {
        if self.busy.get_untracked() {
            self.queued.set(true);
            return;
        }
        self.busy.set(true);
        spawn_local(async move {
            loop {
                self.queued.set(false);
                self.error.set(None);
                let started = mono();
                self.clients_generation.update(|g| *g += 1);
                let clients_generation = self.clients_generation.get_untracked();
                let (cameras, clients, system) = futures::join!(
                    request::<Vec<Camera>>("/cameras", "GET", None),
                    request::<Vec<Client>>("/clients", "GET", None),
                    request::<System>("/system/status", "GET", None)
                );
                if self.alive.try_get_untracked() != Some(true) {
                    return;
                }
                match cameras {
                    Ok(mut cameras) => {
                        if cameras.iter().all(valid_observation) {
                            for camera in &mut cameras {
                                camera.observation_deadline =
                                    started + camera.observation_remaining_ms as f64;
                            }
                            self.cameras.set(cameras)
                        } else {
                            self.error.set(Some(contract_error()))
                        }
                    }
                    Err(e) => self.error.set(Some(e)),
                }
                if self.clients_generation.get_untracked() == clients_generation {
                    match clients {
                        Ok(clients) => self.clients.set(clients),
                        Err(e) => self.error.set(Some(e)),
                    }
                }
                match system {
                    Ok(system) => self.system.set(Some(system)),
                    Err(e) => self.error.set(Some(e)),
                }
                if !self.queued.get_untracked() {
                    break;
                }
            }
            self.busy.set(false);
        });
    }
    fn mutate(
        self,
        path: String,
        method: &'static str,
        body: Option<serde_json::Value>,
        notice: String,
    ) {
        if self.busy.get_untracked() {
            return;
        }
        self.busy.set(true);
        self.error.set(None);
        spawn_local(async move {
            let result = command(&path, method, body).await;
            if self.alive.try_get_untracked() != Some(true) {
                return;
            }
            self.busy.set(false);
            match result {
                Ok(()) => {
                    self.toast(notice);
                    self.refresh()
                }
                Err(error) => self.error.set(Some(error)),
            }
        });
    }
}
fn hash() -> String {
    let value = web_sys::window()
        .unwrap()
        .location()
        .hash()
        .unwrap_or_default();
    match value.trim_start_matches('#') {
        "details" => "details",
        "logs" => "logs",
        "account" => "account",
        _ => "instances",
    }
    .into()
}
pub fn navigate(page: &str) {
    let _ = web_sys::window().unwrap().location().set_hash(page);
}

#[wasm_bindgen(start)]
pub fn start() {
    xcss_web::mount_server("root", || {
        View::new({
            #[allow(unreachable_code)]
            #[allow(unused_mut)]
            #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
            component_view(
                #[allow(clippy::needless_borrows_for_generic_args)]
                &AdminShell,
                {
                    let mut props = component_props_builder(&AdminShell)
                        .product(
                            #[allow(unused_braces)]
                            {
                                "xcos"
                            },
                        )
                        .landing(
                            #[allow(unused_braces)]
                            {
                                "#instances"
                            },
                        )
                        .children({
                            ::xcss_web::children::ToChildren::to_children(move || {
                                #[allow(unreachable_code)]
                                #[allow(unused_mut)]
                                #[allow(
                                    clippy::let_and_return,
                                    clippy::let_unit_value,
                                    clippy::unit_arg
                                )]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &Console,
                                    {
                                        let mut props = component_props_builder(&Console).build();
                                        props
                                    },
                                )
                            })
                        })
                        .build();
                    props
                },
            )
        })
    });
}

#[component]
fn Console() -> impl IntoView {
    let app = AppState {
        cameras: RwSignal::new(vec![]),
        clients: RwSignal::new(vec![]),
        system: RwSignal::new(None),
        selected: RwSignal::new(String::new()),
        clients_generation: RwSignal::new(0),
        error: RwSignal::new(None),
        busy: RwSignal::new(false),
        queued: RwSignal::new(false),
        logs_refresh: RwSignal::new(0),
        alive: RwSignal::new(true),
    };
    provide_context(app);
    let page = RwSignal::new(hash());
    let inspect = RwSignal::new(None::<String>);
    let hash_listener = StoredValue::new_local(gloo_events::EventListener::new(
        &web_sys::window().unwrap(),
        "hashchange",
        move |_| page.set(hash()),
    ));
    let source = web_sys::EventSource::new("/api/v1/events/stream").ok();
    let listeners = StoredValue::new_local(Vec::<gloo_events::EventListener>::new());
    if let Some(source) = &source {
        for kind in ["open", "resync-required", "system-event"] {
            listeners.update_value(|listeners|listeners.push(gloo_events::EventListener::new(source,kind,move|event|{
            if kind=="resync-required"{app.toast(t("事件流曾中断，正在重新同步当前状态","The event stream was interrupted; current state is being resynchronized"));}
            if kind=="system-event"&& let Some(event)=event.dyn_ref::<web_sys::MessageEvent>() {match event.data().as_string().and_then(|s|serde_json::from_str::<serde_json::Value>(&s).ok()){Some(v)=>app.toast(label(v.get("kind").and_then(|v|v.as_str()).unwrap_or_default())),None=>app.toast(t("收到无法解析的事件通知","Received an unreadable event notification"))}}
            app.refresh();app.logs_refresh.update(|v|*v+=1);
        })));
        }
    }
    let source = StoredValue::new_local(source);
    let timer = StoredValue::new_local(gloo_timers::callback::Interval::new(15_000, move || {
        if !web_sys::window().unwrap().document().unwrap().hidden() {
            app.refresh();
        }
    }));
    on_cleanup(move || {
        app.alive.set(false);
        hash_listener.dispose();
        listeners.dispose();
        timer.dispose();
        source.update_value(|s| {
            if let Some(s) = s.take() {
                s.close()
            }
        });
    });
    app.refresh();
    let chosen = Memo::new(move |_| {
        let clients = app.clients.get();
        clients
            .iter()
            .find(|c| c.id == app.selected.get())
            .or(clients.first())
            .cloned()
    });
    let drawer_id = Memo::new(move |_| {
        let id = inspect.get()?;
        app.cameras
            .with(|cameras| cameras.iter().any(|camera| camera.id == id))
            .then_some(id)
    });
    View::new(
    xcss_web::html::div()
        .child((
            {
                #[allow(unreachable_code)] #[allow(unused_mut)]
                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                component_view(
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    &HeaderActions,
                    {
                        let mut props = component_props_builder(
                                &HeaderActions,
                            )
                            .children({
                                ::xcss_web::children::ToChildren::to_children(move || (
                                    xcss_web::html::button()
                                        .child(
                                            #[allow(unused_braces)]
                                            {
                                                IntoRender::into_render({
                                                    t("新建实例", "Create instance")
                                                })
                                            },
                                        )
                                        .class("xcss-button")
                                        .disabled(
                                            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                            app.busy.get()),
                                        )
                                        .on(
                                            xcss_web::ev::click,
                                            move |_| {
                                                navigate("instances");
                                                app.mutate(
                                                    "/clients".into(),
                                                    "POST",
                                                    Some(json!({ "name" : t("新实例", "New instance") })),
                                                    t("客户端实例已创建", "Client instance created"),
                                                )
                                            },
                                        ),
                                    xcss_web::html::button()
                                        .child(
                                            #[allow(unused_braces)]
                                            {
                                                IntoRender::into_render({
                                                    t("刷新", "Refresh")
                                                })
                                            },
                                        )
                                        .class("xcss-button")
                                        .disabled(
                                            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                            app.busy.get()),
                                        )
                                        .on(
                                            xcss_web::ev::click,
                                            move |_| {
                                                app.refresh();
                                                app.logs_refresh.update(|v| *v += 1)
                                            },
                                        ),
                                ))
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
                    &HeaderNavigation,
                    {
                        let mut props = component_props_builder(
                                &HeaderNavigation,
                            )
                            .children({
                                ::xcss_web::children::ToChildren::to_children(move || IntoRender::into_render({
                                    [
                                        ("instances", "实例列表", "Instance list"),
                                        ("details", "详细信息", "Details"),
                                        ("logs", "日志", "Logs"),
                                    ]
                                        .into_iter()
                                        .map(|(id, zh, en)| {
                                            View::new(
    xcss_web::html::button()
        .child(
            #[allow(unused_braces)]
            { IntoRender::into_render({ t(zh, en) }) },
        )
        .class("xcss-button")
        .aria_pressed(
            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move || {
                (page.get() == id).to_string()
            }),
        )
        .disabled(
            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move || {
                id == "details" && chosen.get().is_none()
            }),
        )
        .on(xcss_web::ev::click, move |_| navigate(id)),
)
                                        })
                                        .collect_view()
                                }))
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
                            .error(#[allow(unused_braces)] { app.error })
                            .build();
                        props
                    },
                )
            },
            IntoRender::into_render({
                move || match page.get().as_str() {
                    "account" => {
                        View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &AccountPage,
        {
            let mut props = component_props_builder(
                    &AccountPage,
                )
                .build();
            props
        },
    )
})
                            .into_any()
                    }
                    "details" => {
                        View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &For,
        {
            let mut props = component_props_builder(&For)
                .each(
                    #[allow(unused_braces)]
                    { move || chosen.get().map(|c| vec![c.id]).unwrap_or_default() },
                )
                .key(#[allow(unused_braces)] { |id| id.clone() })
                .children(
                    #[allow(unused_braces)]
                    {
                        move |_| {
                            View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &Details,
        {
            let mut props = component_props_builder(&Details)
                .client(#[allow(unused_braces)] { chosen.get_untracked().unwrap() })
                .inspect(#[allow(unused_braces)] { inspect })
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
})
                            .into_any()
                    }
                    "logs" => {
                        View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &logs::Logs,
        {
            let mut props = component_props_builder(&logs::Logs)
                .build();
            props
        },
    )
})
                            .into_any()
                    }
                    _ => {
                        View::new((
    {
        #[allow(unreachable_code)] #[allow(unused_mut)]
        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
        component_view(
            #[allow(clippy::needless_borrows_for_generic_args)]
            &Statistics,
            {
                let mut props = component_props_builder(
                        &Statistics,
                    )
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
            &Clients,
            {
                let mut props = component_props_builder(
                        &Clients,
                    )
                    .build();
                props
            },
        )
    },
))
                            .into_any()
                    }
                }
            }),
            IntoRender::into_render({
                move || {
                    drawer_id
                        .get()
                        .map(|id| {
                            View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &controls::CameraDrawer,
        {
            let mut props = component_props_builder(
                    &controls::CameraDrawer,
                )
                .id(#[allow(unused_braces)] { id })
                .close(
                    #[allow(unused_braces)]
                    { Callback::new(move |_| inspect.set(None)) },
                )
                .build();
            props
        },
    )
})
                        })
                }
            }),
        ))
        .class("xcos-business xcss-content-stack"),
)
}

#[component]
pub fn Failure(error: RwSignal<Option<xcss_web::api::ApiError>>) -> impl IntoView {
    View::new({
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
                            move || error.get().is_some()
                        },
                    )
                    .children({
                        ::xcss_web::children::ToChildren::to_children(move || {
                            xcss_web::html::div()
                                .child((
                                    xcss_web::html::p().child(
                                        #[allow(unused_braces)]
                                        {
                                            IntoRender::into_render({
                                                move || {
                                                    error
                                                        .get()
                                                        .map(|e| e.to_string())
                                                        .unwrap_or_default()
                                                }
                                            })
                                        },
                                    ),
                                    IntoRender::into_render({
                                        move || {
                                            error.get().and_then(|e| e.request_id).map(|id| {
                                                View::new(xcss_web::html::small().child((
                                                    IntoRender::into_render({
                                                        t("请求标识：", "Request ID: ")
                                                    }),
                                                    IntoRender::into_render(id),
                                                )))
                                            })
                                        }
                                    }),
                                ))
                                .class("xcss-error-state")
                                .role("alert")
                        })
                    })
                    .build();
                props
            },
        )
    })
}
#[component]
fn Statistics() -> impl IntoView {
    let app = expect_context::<AppState>();
    View::new(
        xcss_web::html::section()
            .child(
                #[allow(unused_braces)]
                {
                    xcss_web::html::div()
                        .child(
                            #[allow(unused_braces)]
                            {
                                xcss_web::html::table()
                                    .child((
                                        xcss_web::html::thead().child(
                                            #[allow(unused_braces)]
                                            {
                                                xcss_web::html::tr().child((
                                                    xcss_web::html::th().child(
                                                        #[allow(unused_braces)]
                                                        {
                                                            IntoRender::into_render({
                                                                t("统计项", "Metric")
                                                            })
                                                        },
                                                    ),
                                                    xcss_web::html::th().child(
                                                        #[allow(unused_braces)]
                                                        {
                                                            IntoRender::into_render({
                                                                t("总数 / 在线", "Total / online")
                                                            })
                                                        },
                                                    ),
                                                ))
                                            },
                                        ),
                                        xcss_web::html::tbody().child((
                                            xcss_web::html::tr().child((
                                                xcss_web::html::th()
                                                    .child(
                                                        #[allow(unused_braces)]
                                                        {
                                                            IntoRender::into_render({
                                                                t("总数", "Total")
                                                            })
                                                        },
                                                    )
                                                    .scope("row"),
                                                xcss_web::html::td().child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            move || {
                                                                format!(
                                                                    "{} / {}",
                                                                    app.clients.get().len(),
                                                                    app.clients
                                                                        .get()
                                                                        .iter()
                                                                        .filter(|c| c.status
                                                                            == "online")
                                                                        .count()
                                                                )
                                                            }
                                                        })
                                                    },
                                                ),
                                            )),
                                            xcss_web::html::tr().child((
                                                xcss_web::html::th()
                                                    .child(
                                                        #[allow(unused_braces)]
                                                        {
                                                            IntoRender::into_render({
                                                                t(
                                                                    "已上报摄像机",
                                                                    "Reported cameras",
                                                                )
                                                            })
                                                        },
                                                    )
                                                    .scope("row"),
                                                xcss_web::html::td().child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            move || app.cameras.get().len()
                                                        })
                                                    },
                                                ),
                                            )),
                                            xcss_web::html::tr().child((
                                                xcss_web::html::th()
                                                    .child(
                                                        #[allow(unused_braces)]
                                                        {
                                                            IntoRender::into_render({
                                                                t("媒体服务", "Media service")
                                                            })
                                                        },
                                                    )
                                                    .scope("row"),
                                                xcss_web::html::td().child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            move || {
                                                                app.system
                                                                    .get()
                                                                    .map(|s| {
                                                                        if s.media_service == "ok" {
                                                                            t("正常", "Available")
                                                                        } else {
                                                                            t(
                                                                                "不可用",
                                                                                "Unavailable",
                                                                            )
                                                                        }
                                                                    })
                                                                    .unwrap_or(t(
                                                                        "读取中",
                                                                        "Loading",
                                                                    ))
                                                            }
                                                        })
                                                    },
                                                ),
                                            )),
                                            xcss_web::html::tr().child((
                                                xcss_web::html::th()
                                                    .child(
                                                        #[allow(unused_braces)]
                                                        {
                                                            IntoRender::into_render({
                                                                t(
                                                                    "已配置服务器录像",
                                                                    "Server recordings configured",
                                                                )
                                                            })
                                                        },
                                                    )
                                                    .scope("row"),
                                                xcss_web::html::td().child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            move || {
                                                                app.system
                                                                    .get()
                                                                    .map(|s| {
                                                                        s.cameras
                                                                            .recording_configured
                                                                            .to_string()
                                                                    })
                                                                    .unwrap_or("—".into())
                                                            }
                                                        })
                                                    },
                                                ),
                                            )),
                                        )),
                                    ))
                                    .class("xcss-table xcss-statistics-table")
                                    .aria_label(
                                        ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                            t("实例统计", "Instance statistics"),
                                        ),
                                    )
                            },
                        )
                        .class("xcss-table-scroll")
                },
            )
            .class("view active xcss-content-stack"),
    )
}
#[component]
fn Clients() -> impl IntoView {
    let app = expect_context::<AppState>();
    let candidate = RwSignal::new(String::new());
    let confirm = RwSignal::new(None::<(Client, String)>);
    View::new(
    xcss_web::html::div()
        .child((
            xcss_web::html::div()
                .child(
                    #[allow(unused_braces)]
                    {
                        xcss_web::html::table()
                            .child((
                                xcss_web::html::thead()
                                    .child(
                                        #[allow(unused_braces)]
                                        {
                                            xcss_web::html::tr()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            [
                                                                ("实例名称", "Instance name"),
                                                                ("配对状态", "Pairing status"),
                                                                ("摄像机状态", "Camera status"),
                                                                ("厂商 / 型号", "Make / model"),
                                                                ("操作", "Actions"),
                                                                ("删除", "Delete"),
                                                            ]
                                                                .into_iter()
                                                                .map(|(zh, en)| {
                                                                    View::new(
    xcss_web::html::th()
        .child(
            #[allow(unused_braces)]
            { IntoRender::into_render({ t(zh, en) }) },
        ),
)
                                                                })
                                                                .collect_view()
                                                        })
                                                    },
                                                )
                                        },
                                    ),
                                xcss_web::html::tbody()
                                    .child((
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
                                                            { move || app.clients.get().is_empty() },
                                                        )
                                                        .children({
                                                            ::xcss_web::children::ToChildren::to_children(move || {
                                                                xcss_web::html::tr()
                                                                    .child(
                                                                        #[allow(unused_braces)]
                                                                        {
                                                                            xcss_web::html::td()
                                                                                .child(
                                                                                    #[allow(unused_braces)]
                                                                                    {
                                                                                        IntoRender::into_render({
                                                                                            t("尚无摄像机实例", "No camera instances")
                                                                                        })
                                                                                    },
                                                                                )
                                                                                .class("empty-state")
                                                                                .colspan("6")
                                                                        },
                                                                    )
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
                                                &For,
                                                {
                                                    let mut props = component_props_builder(
                                                            &For,
                                                        )
                                                        .each(
                                                            #[allow(unused_braces)]
                                                            {
                                                                {
                                                                    move || {
                                                                        app
                                                                            .clients
                                                                            .get()
                                                                            .into_iter()
                                                                            .map(|c| c.id)
                                                                            .collect::<Vec<_>>()
                                                                    }
                                                                }
                                                            },
                                                        )
                                                        .key(#[allow(unused_braces)] { |id| id.clone() })
                                                        .children(
                                                            #[allow(unused_braces)]
                                                            {
                                                                move |id| {
                                                                    let cid = id.clone();
                                                                    let client = Memo::new(move |_| {
                                                                        app.clients.get().into_iter().find(|c| c.id == cid)
                                                                    });
                                                                    let camera = Memo::new(move |_| {
                                                                        app
                                                                            .cameras
                                                                            .get()
                                                                            .into_iter()
                                                                            .find(|c| c.client_id.as_deref() == Some(id.as_str()))
                                                                    });
                                                                    View::new(
    xcss_web::html::tr()
        .child((
            xcss_web::html::th()
                .child(
                    #[allow(unused_braces)]
                    {
                        xcss_web::html::a()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        move || client.get().map(|c| c.name).unwrap_or_default()
                                    })
                                },
                            )
                            .class("xcss-instance-link")
                            .href("#details")
                            .aria_label(
                                ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                {
                                    format!(
                                        "{} {}", t("选择实例", "Select instance"), client.get()
                                        .map(| c | c.name).unwrap_or_default()
                                    )
                                }),
                            )
                            .on(
                                xcss_web::ev::click,
                                move |_| {
                                    if let Some(c) = client.get_untracked() {
                                        app.selected.set(c.id)
                                    }
                                },
                            )
                    },
                )
                .scope("row"),
            xcss_web::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || client.get().map(|c| label(&c.status))
                        })
                    },
                ),
            xcss_web::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || {
                                camera
                                    .get()
                                    .map(|_| {
                                        View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &CameraStatus,
        {
            let mut props = component_props_builder(
                    &CameraStatus,
                )
                .camera(
                    #[allow(unused_braces)]
                    { Signal::derive(move || camera.get()) },
                )
                .build();
            props
        },
    )
})
                                    })
                                    .map(|view| view.into_any())
                                    .unwrap_or_else(|| {
                                        t("尚未上报", "Not yet reported").into_any()
                                    })
                            }
                        })
                    },
                ),
            xcss_web::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || {
                                camera.get().map(|c| make(&c)).unwrap_or("—".into())
                            }
                        })
                    },
                ),
            xcss_web::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        xcss_web::html::div()
                            .child(
                                #[allow(unused_braces)]
                                {
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
                                                            move || client.get().is_some_and(|c| c.status != "revoked")
                                                        },
                                                    )
                                                    .children({
                                                        ::xcss_web::children::ToChildren::to_children(move || (
                                                            xcss_web::html::button()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            t("更换密码", "Change password")
                                                                        })
                                                                    },
                                                                )
                                                                .class("xcss-button")
                                                                .disabled(
                                                                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                                                    app.busy.get()),
                                                                )
                                                                .on(
                                                                    xcss_web::ev::click,
                                                                    move |_| {
                                                                        if let Some(c) = client.get_untracked() {
                                                                            confirm.set(Some((c, "rotate".into())))
                                                                        }
                                                                    },
                                                                ),
                                                            xcss_web::html::button()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            move || {
                                                                                if client.get().is_some_and(|c| c.status == "pending") {
                                                                                    t("取消配对", "Cancel pairing")
                                                                                } else {
                                                                                    t("撤销实例", "Revoke instance")
                                                                                }
                                                                            }
                                                                        })
                                                                    },
                                                                )
                                                                .class("xcss-button")
                                                                .disabled(
                                                                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                                                    app.busy.get()),
                                                                )
                                                                .on(
                                                                    xcss_web::ev::click,
                                                                    move |_| {
                                                                        if let Some(c) = client.get_untracked() {
                                                                            confirm.set(Some((c, "revoke".into())))
                                                                        }
                                                                    },
                                                                ),
                                                        ))
                                                    })
                                                    .build();
                                                props
                                            },
                                        )
                                    }
                                },
                            )
                            .class("xcss-actions")
                    },
                ),
            xcss_web::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        xcss_web::html::div()
                            .child(
                                #[allow(unused_braces)]
                                {
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
                                                                client.get().is_some_and(|c| candidate.get() == c.id)
                                                            }
                                                        },
                                                    )
                                                    .fallback(
                                                        #[allow(unused_braces)]
                                                        {
                                                            move || {
                                                                View::new(
    xcss_web::html::button()
        .child(
            #[allow(unused_braces)]
            {
                IntoRender::into_render({
                    t("永久删除", "Permanently delete")
                })
            },
        )
        .class("xcss-button")
        .disabled(
            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move || {
                app.busy.get() || client.get().is_none_or(|c| c.status != "revoked")
            }),
        )
        .on(
            xcss_web::ev::click,
            move |_| {
                if let Some(c) = client.get_untracked() {
                    candidate.set(c.id)
                }
            },
        ),
)
                                                            }
                                                        },
                                                    )
                                                    .children({
                                                        ::xcss_web::children::ToChildren::to_children(move || (
                                                            xcss_web::html::button()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            t("取消", "Cancel")
                                                                        })
                                                                    },
                                                                )
                                                                .class("xcss-button")
                                                                .on(
                                                                    xcss_web::ev::click,
                                                                    move |_| candidate.set(String::new()),
                                                                ),
                                                            xcss_web::html::button()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            t("确认删除", "Confirm delete")
                                                                        })
                                                                    },
                                                                )
                                                                .class("xcss-button xcss-danger")
                                                                .disabled(
                                                                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                                                    app.busy.get()),
                                                                )
                                                                .on(
                                                                    xcss_web::ev::click,
                                                                    move |_| {
                                                                        if let Some(c) = client.get_untracked()
                                                                            && c.status == "revoked"
                                                                        {
                                                                            app.mutate(
                                                                                format!("/clients/{}/delete", c.id),
                                                                                "DELETE",
                                                                                None,
                                                                                t("实例已永久删除", "Instance permanently deleted"),
                                                                            );
                                                                            candidate.set(String::new());
                                                                        }
                                                                    },
                                                                ),
                                                        ))
                                                    })
                                                    .build();
                                                props
                                            },
                                        )
                                    }
                                },
                            )
                            .class("xcss-actions")
                    },
                ),
        )),
)
                                                                }
                                                            },
                                                        )
                                                        .build();
                                                    props
                                                },
                                            )
                                        },
                                    )),
                            ))
                            .class("xcss-table")
                            .aria_label(
                                ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                    t("摄像机实例列表", "Camera instance list"),
                                ),
                            )
                    },
                )
                .class("xcss-table-scroll"),
            IntoRender::into_render({
                move || {
                    confirm
                        .get()
                        .map(|(client, action)| {
                            let rotate = action == "rotate";
                            View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &Confirm,
        {
            let mut props = component_props_builder(&Confirm)
                .title(
                    #[allow(unused_braces)]
                    {
                        if rotate {
                            t("更换密码", "Change password")
                        } else {
                            t("撤销实例", "Revoke instance")
                        }
                    },
                )
                .description(
                    #[allow(unused_braces)]
                    {
                        if rotate {
                            t(
                                "这会立即撤销当前客户端凭据并停用它管理的摄像头。客户端必须使用新密码重新配对并重新上报摄像头。",
                                "This immediately revokes the current client credential and disables its cameras. The client must pair again with the new password and report its cameras again.",
                            )
                        } else {
                            t(
                                "当前密码和客户端凭据将失效；重复撤销不会删除实例信息。媒体路径清理完成后，需单独确认永久删除。",
                                "The password and client credential will be invalidated. Repeating revocation preserves the instance entry. After media-path cleanup, permanent deletion requires a separate confirmation.",
                            )
                        }
                    },
                )
                .close(
                    #[allow(unused_braces)]
                    { Callback::new(move |_| confirm.set(None)) },
                )
                .accept(
                    #[allow(unused_braces)]
                    {
                        Callback::new(move |_| {
                            confirm.set(None);
                            if rotate {
                                app.mutate(
                                    format!("/clients/{}/authorization", client.id),
                                    "PUT",
                                    Some(json!({ "authorization_code" : random_code() })),
                                    t(
                                        "实例密码已更换；原客户端必须使用新密码重新配对",
                                        "Instance password changed; the old client must pair again with the new password",
                                    ),
                                );
                            } else {
                                app.mutate(
                                    format!("/clients/{}", client.id),
                                    "DELETE",
                                    None,
                                    t(
                                        "实例已撤销；媒体路径清理完成后，可单独确认永久删除",
                                        "Instance revoked; after media-path cleanup, confirm permanent deletion separately",
                                    ),
                                );
                            }
                        })
                    },
                )
                .build();
            props
        },
    )
})
                        })
                }
            }),
        ))
        .class("xcss-content-stack"),
)
}

#[component]
pub fn Confirm(
    title: String,
    description: String,
    close: Callback<()>,
    accept: Callback<()>,
) -> impl IntoView {
    let dialog = NodeRef::<xcss_web::html::Dialog>::new();
    Effect::new(move |_| {
        if let Some(dialog) = dialog.get() {
            let _ = dialog.show_modal();
        }
    });
    View::new(
        xcss_web::html::dialog()
            .child((
                xcss_web::html::h2().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({ title.clone() })
                    },
                ),
                xcss_web::html::p().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({ description })
                    },
                ),
                xcss_web::html::div()
                    .child((
                        xcss_web::html::button()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({ t("取消", "Cancel") })
                                },
                            )
                            .class("xcss-button")
                            .on(xcss_web::ev::click, move |_| close.run(())),
                        xcss_web::html::button()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({ t("确认", "Confirm") })
                                },
                            )
                            .class("xcss-button xcss-danger")
                            .on(xcss_web::ev::click, move |_| accept.run(())),
                    ))
                    .class("xcss-actions"),
            ))
            .class("xcss-dialog")
            .node_ref(dialog)
            .on(
                xcss_web::ev::Custom::new("cancel"),
                move |event: web_sys::Event| {
                    event.prevent_default();
                    close.run(());
                },
            )
            .aria_modal("true")
            .aria_label(
                ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(title.clone()),
            ),
    )
}
fn make(c: &Camera) -> String {
    let text = [
        c.manufacturer.as_deref().unwrap_or_default(),
        c.model.as_deref().unwrap_or_default(),
    ]
    .into_iter()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" / ");
    if text.is_empty() {
        c.adapter_kind.to_uppercase()
    } else {
        text
    }
}

#[component]
fn Details(client: Client, inspect: RwSignal<Option<String>>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let id = client.id.clone();
    let current = Memo::new(move |_| app.clients.get().into_iter().find(|c| c.id == id));
    let id = client.id.clone();
    let cameras = Memo::new(move |_| {
        app.cameras
            .get()
            .into_iter()
            .filter(|c| c.client_id.as_deref() == Some(id.as_str()))
            .collect::<Vec<_>>()
    });
    let search = RwSignal::new(String::new());
    let visible = Memo::new(move |_| {
        let term = search.get().trim().to_lowercase();
        cameras
            .get()
            .into_iter()
            .filter(|c| {
                format!("{} {}", c.name, c.location)
                    .to_lowercase()
                    .contains(&term)
            })
            .collect::<Vec<_>>()
    });
    View::new(
    xcss_web::html::section()
        .child((
            xcss_web::html::section()
                .child((
                    xcss_web::html::h2()
                        .child(
                            #[allow(unused_braces)]
                            {
                                IntoRender::into_render({
                                    move || current.get().map(|c| c.name)
                                })
                            },
                        ),
                    xcss_web::html::dl()
                        .child((
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("账户名", "Account name")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || current.get().map(|c| c.name)
                                        })
                                    },
                                ),
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("账户", "Account")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        xcss_web::html::code()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        client.id.clone()
                                                    })
                                                },
                                            )
                                    },
                                ),
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("密码", "Password")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        xcss_web::html::code()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        move || current.get().map(|c| c.authorization_code)
                                                    })
                                                },
                                            )
                                    },
                                ),
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("配对状态", "Pairing status")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || {
                                                current
                                                    .get()
                                                    .map(|c| {
                                                        if c.status == "pending" {
                                                            t("待配对", "Awaiting pairing")
                                                        } else if c.status == "revoked" {
                                                            t("已撤销", "Revoked")
                                                        } else {
                                                            t("已配对", "Paired")
                                                        }
                                                    })
                                            }
                                        })
                                    },
                                ),
                        ))
                        .class("xcos-detail-list"),
                ))
                .class("xcss-content-panel")
                .aria_label(
                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                        t("配对账户信息", "Pairing account information"),
                    ),
                ),
            xcss_web::html::section()
                .child((
                    xcss_web::html::h2()
                        .child(
                            #[allow(unused_braces)]
                            {
                                IntoRender::into_render({
                                    t("摄像机状态", "Camera status")
                                })
                            },
                        ),
                    xcss_web::html::dl()
                        .child((
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("客户端连接", "Client connection")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || current.get().map(|c| status_label(&c.status))
                                        })
                                    },
                                ),
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("版本", "Version")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || {
                                                current
                                                    .get()
                                                    .and_then(|c| c.client_version)
                                                    .unwrap_or("—".into())
                                            }
                                        })
                                    },
                                ),
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("最后在线", "Last online")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || {
                                                current
                                                    .get()
                                                    .and_then(|c| c.last_seen_at)
                                                    .map(|v| date(&v))
                                                    .unwrap_or("—".into())
                                            }
                                        })
                                    },
                                ),
                            xcss_web::html::dt()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("摄像头", "Cameras")
                                        })
                                    },
                                ),
                            xcss_web::html::dd()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || cameras.get().len()
                                        })
                                    },
                                ),
                        ))
                        .class("xcos-detail-list"),
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
                                            {
                                                move || {
                                                    cameras.get().into_iter().map(|c| c.id).collect::<Vec<_>>()
                                                }
                                            }
                                        },
                                    )
                                    .key(#[allow(unused_braces)] { |id| id.clone() })
                                    .children(
                                        #[allow(unused_braces)]
                                        {
                                            move |id| {
                                                let camera = Signal::derive(move || {
                                                    app.cameras.get().into_iter().find(|c| c.id == id)
                                                });
                                                View::new(
    xcss_web::html::section()
        .child((
            xcss_web::html::h3()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || camera.get().map(|c| c.name)
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
        )),
)
                                            }
                                        },
                                    )
                                    .build();
                                props
                            },
                        )
                    },
                ))
                .class("xcss-content-panel")
                .aria_label(
                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                        t("摄像机状态", "Camera status"),
                    ),
                ),
            xcss_web::html::div()
                .child(
                    #[allow(unused_braces)]
                    {
                        xcss_web::html::div()
                            .child((
                                xcss_web::html::span()
                                    .child(#[allow(unused_braces)] { "⌕" }),
                                xcss_web::html::input()
                                    .class("xcss-input")
                                    .r#type("search")
                                    .aria_label(
                                        ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                            t("搜索摄像头", "Search cameras"),
                                        ),
                                    )
                                    .placeholder(
                                        ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                                            t("搜索名称或位置", "Search by name or location"),
                                        ),
                                    )
                                    .prop("value", move || search.get())
                                    .on(
                                        xcss_web::ev::input,
                                        move |e| search.set(event_target_value(&e)),
                                    ),
                            ))
                            .class("search-wrap")
                    },
                )
                .class("command-bar"),
            xcss_web::html::div()
                .child((
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
                                        { move || visible.get().is_empty() },
                                    )
                                    .children({
                                        ::xcss_web::children::ToChildren::to_children(move || {
                                            xcss_web::html::div()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            t(
                                                                "该客户端还没有上报匹配的摄像头。",
                                                                "This client has not reported any matching cameras yet.",
                                                            )
                                                        })
                                                    },
                                                )
                                                .class("empty-state full-span")
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
                            &For,
                            {
                                let mut props = component_props_builder(
                                        &For,
                                    )
                                    .each(
                                        #[allow(unused_braces)]
                                        {
                                            {
                                                move || {
                                                    visible
                                                        .get()
                                                        .into_iter()
                                                        .map(|c| (c.id, c.enabled, c.has_sub_stream))
                                                        .collect::<Vec<_>>()
                                                }
                                            }
                                        },
                                    )
                                    .key(
                                        #[allow(unused_braces)]
                                        { |(id, enabled, sub)| (id.clone(), *enabled, *sub) },
                                    )
                                    .children(
                                        #[allow(unused_braces)]
                                        {
                                            move |(id, _, _)| {
                                                let cid = id.clone();
                                                let camera = Signal::derive(move || {
                                                    app.cameras.get().into_iter().find(|c| c.id == cid)
                                                });
                                                let initial = camera.get_untracked().unwrap();
                                                View::new(
    xcss_web::html::article()
        .child((
            {
                #[allow(unreachable_code)] #[allow(unused_mut)]
                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                component_view(
                    #[allow(clippy::needless_borrows_for_generic_args)]
                    &media::LiveVideo,
                    {
                        let mut props = component_props_builder(
                                &media::LiveVideo,
                            )
                            .camera(#[allow(unused_braces)] { initial.clone() })
                            .profile(
                                #[allow(unused_braces)]
                                {
                                    if initial.has_sub_stream {
                                        "sub".into()
                                    } else {
                                        "main".into()
                                    }
                                },
                            )
                            .build();
                        props
                    },
                )
            },
            xcss_web::html::div()
                .child((
                    xcss_web::html::div()
                        .child((
                            xcss_web::html::strong()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || camera.get().map(|c| c.name)
                                        })
                                    },
                                ),
                            xcss_web::html::small()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || {
                                                camera
                                                    .get()
                                                    .map(|c| {
                                                        format!(
                                                            "{} · {} · {}", if c.location.is_empty() {
                                                            t("未标注位置", "Location not specified") } else { c
                                                            .location.clone() }, make(& c), if c.storage_mode ==
                                                            "server" { t("服务器录像", "Server recording") } else {
                                                            t("客户端录像", "Client recording") }
                                                        )
                                                    })
                                            }
                                        })
                                    },
                                ),
                        )),
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
                ))
                .class("camera-meta"),
            xcss_web::html::div()
                .child(
                    #[allow(unused_braces)]
                    {
                        xcss_web::html::button()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        t("查看与控制", "View and control")
                                    })
                                },
                            )
                            .class("xcss-button")
                            .on(
                                xcss_web::ev::click,
                                move |_| inspect.set(Some(id.clone())),
                            )
                    },
                )
                .class("camera-actions"),
        ))
        .class("camera-card xcss-content-panel reveal"),
)
                                            }
                                        },
                                    )
                                    .build();
                                props
                            },
                        )
                    },
                ))
                .class("camera-grid"),
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
                                        cameras.get().iter().any(|c| c.storage_mode == "server")
                                    }
                                },
                            )
                            .children({
                                ::xcss_web::children::ToChildren::to_children(move || {
                                    #[allow(unreachable_code)] #[allow(unused_mut)]
                                    #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                    component_view(
                                        #[allow(clippy::needless_borrows_for_generic_args)]
                                        &recordings::Recordings,
                                        {
                                            let mut props = component_props_builder(
                                                    &recordings::Recordings,
                                                )
                                                .cameras(
                                                    #[allow(unused_braces)]
                                                    {
                                                        Signal::derive(move || {
                                                            cameras
                                                                .get()
                                                                .into_iter()
                                                                .filter(|c| c.storage_mode == "server")
                                                                .collect()
                                                        })
                                                    },
                                                )
                                                .build();
                                            props
                                        },
                                    )
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
                    &ClientSettings,
                    {
                        let mut props = component_props_builder(
                                &ClientSettings,
                            )
                            .client(
                                #[allow(unused_braces)]
                                { Signal::derive(move || current.get()) },
                            )
                            .build();
                        props
                    },
                )
            },
        ))
        .class("view active xcss-content-stack"),
)
}
#[component]
fn ClientSettings(client: Signal<Option<Client>>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let name = RwSignal::new(client.get_untracked().map(|c| c.name).unwrap_or_default());
    let synced = RwSignal::new(name.get_untracked());
    let pending = RwSignal::new(false);
    let failure = RwSignal::new(None);
    let saved = RwSignal::new(false);
    Effect::new(move |_| {
        if let Some(c) = client.get() {
            let previous = synced.get_untracked();
            synced.set(c.name.clone());
            if name.get_untracked() == previous {
                name.set(c.name)
            }
            failure.set(None);
        }
    });
    let refresh_saved = move || {
        pending.set(true);
        failure.set(None);
        app.clients_generation.update(|g| *g += 1);
        let generation = app.clients_generation.get_untracked();
        spawn_local(async move {
            let result = request::<Vec<Client>>("/clients", "GET", None).await;
            if pending.is_disposed() {
                return;
            }
            pending.set(false);
            match result {
                Ok(clients) => {
                    if app.clients_generation.get_untracked() == generation {
                        app.clients.set(clients);
                    }
                    failure.set(None);
                    app.toast(t("实例名称已保存", "Instance name saved"));
                }
                Err(error) => failure.set(Some(error)),
            }
        });
    };
    View::new(
    xcss_web::html::section()
        .child((
            xcss_web::html::h2()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            t("实例设置", "Instance settings")
                        })
                    },
                ),
            xcss_web::html::form()
                .child((
                    xcss_web::html::label()
                        .child((
                            IntoRender::into_render({
                                t("实例名称", "Instance name")
                            }),
                            xcss_web::html::input()
                                .class("xcss-input")
                                .name("name")
                                .required(true)
                                .readonly(
                                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                    {
                                        pending.get()
                                            || client.get().is_some_and(|c| c.status == "revoked")
                                    }),
                                )
                                .prop("value", move || name.get())
                                .on(
                                    xcss_web::ev::input,
                                    move |e| name.set(event_target_value(&e)),
                                ),
                        ))
                        .class("xcss-form-field"),
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
                                            move || client.get().is_some_and(|c| c.status == "revoked")
                                        },
                                    )
                                    .children({
                                        ::xcss_web::children::ToChildren::to_children(move || {
                                            xcss_web::html::p()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            t(
                                                                "已撤销实例不能更改名称；媒体路径清理完成后可永久删除。",
                                                                "A revoked instance cannot be renamed; it can be permanently deleted after media-path cleanup.",
                                                            )
                                                        })
                                                    },
                                                )
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
                            &Show,
                            {
                                let mut props = component_props_builder(
                                        &Show,
                                    )
                                    .when(
                                        #[allow(unused_braces)]
                                        { move || !valid_name(&name.get()) },
                                    )
                                    .children({
                                        ::xcss_web::children::ToChildren::to_children(move || {
                                            xcss_web::html::p()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            t(
                                                                "名称须为 1–64 个字符，不能包含控制字符。",
                                                                "Use 1–64 characters without control characters.",
                                                            )
                                                        })
                                                    },
                                                )
                                                .role("alert")
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
                                    .error(#[allow(unused_braces)] { failure })
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
                                        { move || saved.get() && failure.get().is_some() },
                                    )
                                    .children({
                                        ::xcss_web::children::ToChildren::to_children(move || {
                                            xcss_web::html::p()
                                                .child((
                                                    IntoRender::into_render({
                                                        t(
                                                            "实例名称已保存，但列表刷新失败。请重试刷新。",
                                                            "The instance name was saved, but the list could not be refreshed. Please retry the refresh.",
                                                        )
                                                    }),
                                                    xcss_web::html::button()
                                                        .child(
                                                            #[allow(unused_braces)]
                                                            {
                                                                IntoRender::into_render({
                                                                    t("重试刷新", "Retry refresh")
                                                                })
                                                            },
                                                        )
                                                        .class("xcss-button")
                                                        .r#type("button")
                                                        .on(
                                                            xcss_web::ev::click,
                                                            move |_| refresh_saved(),
                                                        ),
                                                ))
                                                .role("alert")
                                        })
                                    })
                                    .build();
                                props
                            },
                        )
                    },
                    xcss_web::html::div()
                        .child(
                            #[allow(unused_braces)]
                            {
                                xcss_web::html::button()
                                    .child(
                                        #[allow(unused_braces)]
                                        {
                                            IntoRender::into_render({
                                                move || {
                                                    if pending.get() {
                                                        t("正在保存…", "Saving…")
                                                    } else {
                                                        t("保存名称", "Save name")
                                                    }
                                                }
                                            })
                                        },
                                    )
                                    .class("xcss-button")
                                    .r#type("submit")
                                    .disabled(
                                        ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                                        {
                                            pending.get()
                                                || client.get().is_none_or(|c| c.status == "revoked")
                                                || !valid_name(&name.get())
                                                || name.get().trim_matches(char::is_whitespace)
                                                    == synced.get().trim_matches(char::is_whitespace)
                                        }),
                                    )
                            },
                        )
                        .class("xcss-actions"),
                ))
                .on(
                    xcss_web::ev::submit,
                    move |e| {
                        e.prevent_default();
                        if pending.get_untracked() {
                            return;
                        }
                        let Some(c) = client.get_untracked() else { return };
                        let normalized = name
                            .get_untracked()
                            .trim_matches(char::is_whitespace)
                            .to_string();
                        if !valid_name(&normalized) || c.status == "revoked"
                            || normalized == synced.get_untracked()
                        {
                            return;
                        }
                        pending.set(true);
                        failure.set(None);
                        saved.set(false);
                        spawn_local(async move {
                            let result = request::<
                                Client,
                            >(
                                    &format!("/clients/{}", c.id),
                                    "PATCH",
                                    Some(json!({ "name" : normalized })),
                                )
                                .await;
                            if pending.is_disposed() {
                                return;
                            }
                            pending.set(false);
                            match result {
                                Ok(updated) => {
                                    synced.set(updated.name);
                                    saved.set(true);
                                    refresh_saved();
                                }
                                Err(e) => failure.set(Some(e)),
                            }
                        });
                    },
                )
                .aria_busy(
                    ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(move ||
                    pending.get().to_string()),
                ),
        ))
        .class("xcss-content-panel")
        .aria_label(
            ::xcss_web::prelude::IntoAttributeValue::into_attribute_value(
                t("实例设置", "Instance settings"),
            ),
        ),
)
}
#[component]
pub fn CameraStatus(camera: Signal<Option<Camera>>) -> impl IntoView {
    let tick = RwSignal::new(mono());
    let timer = StoredValue::new_local(None::<gloo_timers::callback::Timeout>);
    Effect::new(move |_| {
        let current = camera.get();
        tick.track();
        timer.update_value(|timer| {
            timer.take();
        });
        if let Some(camera) = current {
            let delay = camera.observation_deadline - mono();
            if camera.observation_status == "fresh"
                && camera.observation_expires_at.is_some()
                && delay > 0.
            {
                timer.set_value(Some(gloo_timers::callback::Timeout::new(
                    (delay + 1.).min(2_147_483_647.) as u32,
                    move || tick.set(mono()),
                )));
            }
        }
    });
    on_cleanup(move || timer.dispose());
    View::new(
        xcss_web::html::span()
            .child(
                #[allow(unused_braces)]
                {
                    IntoRender::into_render({
                        move || {
                            tick.track();
                            camera
                                .get()
                                .map(|c| status_label(status(&c, mono())))
                                .unwrap_or_default()
                        }
                    })
                },
            )
            .class("camera-status"),
    )
}
#[component]
pub fn Observation(camera: Signal<Option<Camera>>) -> impl IntoView {
    View::new(
        xcss_web::html::dl()
            .child((
                xcss_web::html::dt().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            t("最后有效媒体观测", "Last valid media observation")
                        })
                    },
                ),
                xcss_web::html::dd().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || {
                                camera
                                    .get()
                                    .and_then(|c| c.last_observed_at)
                                    .map(|s| date(&s))
                                    .unwrap_or(t("尚无", "None yet"))
                            }
                        })
                    },
                ),
                xcss_web::html::dt().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            t("最近保存的媒体状态", "Last stored media state")
                        })
                    },
                ),
                xcss_web::html::dd().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || camera.get().map(|c| status_label(&c.status))
                        })
                    },
                ),
                xcss_web::html::dt().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            t(
                                "客户端最近上报的设备状态",
                                "Last client-reported device state",
                            )
                        })
                    },
                ),
                xcss_web::html::dd().child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || camera.get().map(|c| status_label(&c.device_status))
                        })
                    },
                ),
            ))
            .class("xcos-detail-list"),
    )
}
