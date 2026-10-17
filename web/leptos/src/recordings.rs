use crate::{Failure, model::*};
use leptos::component::{component_props_builder, component_view};
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;
use xcss_web::i18n::t;

fn datetime(ms: f64) -> String {
    js_sys::Date::new(&ms.into())
        .to_iso_string()
        .as_string()
        .unwrap_or_default()
        .chars()
        .take(16)
        .collect()
}
#[component]
pub fn Recordings(cameras: Signal<Vec<Camera>>) -> impl IntoView {
    let camera = RwSignal::new(
        cameras
            .get_untracked()
            .first()
            .map(|c| c.id.clone())
            .unwrap_or_default(),
    );
    let start = RwSignal::new(datetime(js_sys::Date::now() - 86_400_000.));
    let end = RwSignal::new(datetime(js_sys::Date::now()));
    let spans = RwSignal::new(Vec::<Span>::new());
    let selected = RwSignal::new(None::<Span>);
    let attempt = RwSignal::new(0u64);
    let generation = RwSignal::new(0u64);
    let state = RwSignal::new("idle");
    let error = RwSignal::new(None);
    let query = Memo::new(move |_| (camera.get(), start.get(), end.get()));
    let loaded_query = RwSignal::new(None::<(String, String, String)>);
    Effect::new(move |_| {
        let cameras = cameras.get();
        if !cameras.iter().any(|c| c.id == camera.get_untracked()) {
            camera.set(cameras.first().map(|c| c.id.clone()).unwrap_or_default());
        }
    });
    Effect::new(move |_| {
        query.track();
        generation.update(|g| *g += 1);
        selected.set(None);
        spans.set(vec![]);
        error.set(None);
        state.set("idle");
    });
    let search = move || {
        let identity = query.get_untracked();
        if identity.0.is_empty() {
            return;
        }
        generation.update(|g| *g += 1);
        let g = generation.get_untracked();
        loaded_query.set(Some(identity.clone()));
        spans.set(vec![]);
        selected.set(None);
        state.set("loading");
        error.set(None);
        spawn_local(async move {
            let start_date = js_sys::Date::new(&format!("{}Z", identity.1).into());
            let end_date = js_sys::Date::new(&format!("{}Z", identity.2).into());
            let result = if !start_date.get_time().is_finite()
                || !end_date.get_time().is_finite()
                || start_date.get_time() > end_date.get_time()
            {
                Err(contract_error())
            } else {
                request::<Vec<Span>>(
                    &format!(
                        "/recordings?camera_id={}&start={}&end={}",
                        encode(&identity.0),
                        encode(&start_date.to_iso_string().as_string().unwrap()),
                        encode(&end_date.to_iso_string().as_string().unwrap())
                    ),
                    "GET",
                    None,
                )
                .await
            };
            if generation.try_get_untracked() != Some(g)
                || query.try_get_untracked() != Some(identity)
            {
                return;
            }
            match result {
                Ok(next) => {
                    spans.set(next);
                    state.set("ready")
                }
                Err(e) => {
                    error.set(Some(e));
                    state.set("failed")
                }
            }
        });
    };
    let current = move || {
        loaded_query.get().as_ref() == Some(&query.get())
            && cameras.get().iter().any(|c| c.id == camera.get())
    };
    let playback = Memo::new(move |_| {
        if !current() {
            return None;
        }
        selected.get().map(|span| {
            format!(
                "/api/v1/recordings/play?camera_id={}&start={}&duration={}&format=fmp4",
                encode(&camera.get()),
                encode(&span.start),
                span.duration
            )
        })
    });
    View::new(
    leptos::html::section()
        .child((
            leptos::html::div()
                .child((
                    leptos::html::label()
                        .child((
                            IntoRender::into_render({
                                t("摄像头", "Camera")
                            }),
                            {
                                #[allow(unreachable_code)] #[allow(unused_mut)]
                                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &CameraSelect,
                                    {
                                        let mut props = component_props_builder(
                                                &CameraSelect,
                                            )
                                            .cameras(#[allow(unused_braces)] { cameras })
                                            .selected(#[allow(unused_braces)] { camera })
                                            .build();
                                        props
                                    },
                                )
                            },
                        )),
                    leptos::html::label()
                        .child((
                            IntoRender::into_render({
                                t("开始时间（UTC）", "Start time (UTC)")
                            }),
                            leptos::html::input()
                                .class("xcss-input")
                                .r#type("datetime-local")
                                .prop("value", move || start.get())
                                .on(
                                    leptos::ev::input,
                                    move |e| start.set(event_target_value(&e)),
                                ),
                        )),
                    leptos::html::label()
                        .child((
                            IntoRender::into_render({
                                t("结束时间（UTC）", "End time (UTC)")
                            }),
                            leptos::html::input()
                                .class("xcss-input")
                                .r#type("datetime-local")
                                .prop("value", move || end.get())
                                .on(
                                    leptos::ev::input,
                                    move |e| end.set(event_target_value(&e)),
                                ),
                        )),
                    leptos::html::button()
                        .child(
                            #[allow(unused_braces)]
                            {
                                IntoRender::into_render({
                                    t("查询录像", "Search recordings")
                                })
                            },
                        )
                        .class("xcss-button button button-primary")
                        .on(leptos::ev::click, move |_| search()),
                ))
                .class("filter-panel xcss-content-panel"),
            leptos::html::div()
                .child((
                    leptos::html::div()
                        .child((
                            leptos::html::div()
                                .child((
                                    leptos::html::h3()
                                        .child(
                                            #[allow(unused_braces)]
                                            {
                                                IntoRender::into_render({
                                                    t("录像时间段", "Recording segments")
                                                })
                                            },
                                        ),
                                    leptos::html::span()
                                        .child((
                                            IntoRender::into_render({
                                                move || if current() { spans.get().len() } else { 0 }
                                            }),
                                            " ",
                                            IntoRender::into_render({
                                                t("条", "segments")
                                            }),
                                        )),
                                ))
                                .class("section-heading"),
                            leptos::html::div()
                                .child((
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
                                                        { move || state.get() == "loading" },
                                                    )
                                                    .children({
                                                        ::leptos::children::ToChildren::to_children(move || {
                                                            leptos::html::p()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            t("正在查询录像…", "Loading recordings…")
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
                                            &Show,
                                            {
                                                let mut props = component_props_builder(
                                                        &Show,
                                                    )
                                                    .when(
                                                        #[allow(unused_braces)]
                                                        { move || !current() || state.get() == "idle" },
                                                    )
                                                    .children({
                                                        ::leptos::children::ToChildren::to_children(move || {
                                                            leptos::html::p()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            t(
                                                                                "选择时间范围后查询录像。",
                                                                                "Choose a time range and search for recordings.",
                                                                            )
                                                                        })
                                                                    },
                                                                )
                                                                .class("empty-state")
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
                                                        {
                                                            move || {
                                                                current() && state.get() == "ready"
                                                                    && spans.get().is_empty()
                                                            }
                                                        },
                                                    )
                                                    .children({
                                                        ::leptos::children::ToChildren::to_children(move || {
                                                            leptos::html::p()
                                                                .child(
                                                                    #[allow(unused_braces)]
                                                                    {
                                                                        IntoRender::into_render({
                                                                            t(
                                                                                "所选范围内没有录像",
                                                                                "No recordings in the selected range",
                                                                            )
                                                                        })
                                                                    },
                                                                )
                                                                .class("empty-state")
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
                                                        { move || if current() { spans.get() } else { vec![] } },
                                                    )
                                                    .key(
                                                        #[allow(unused_braces)]
                                                        { |span| (span.start.clone(), span.duration.to_bits()) },
                                                    )
                                                    .children(
                                                        #[allow(unused_braces)]
                                                        {
                                                            move |span| {
                                                                let playing = span.clone();
                                                                View::new(
    leptos::html::button()
        .child((
            leptos::html::span()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            date(&span.start)
                        })
                    },
                ),
            leptos::html::strong()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            duration(span.duration)
                        })
                    },
                ),
            leptos::html::i()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            t("播放", "Play")
                        })
                    },
                ),
        ))
        .class("xcss-button record-item")
        .on(
            leptos::ev::click,
            move |_| {
                selected.set(Some(playing.clone()));
                attempt.update(|v| *v += 1);
            },
        ),
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
                                .class("record-list"),
                        )),
                    leptos::html::div()
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
                                                { { move || vec![(playback.get(), attempt.get())] } },
                                            )
                                            .key(#[allow(unused_braces)] { |key| key.clone() })
                                            .children(
                                                #[allow(unused_braces)]
                                                {
                                                    move |(source, _)| {
                                                        View::new(
    leptos::html::video()
        .src(::leptos::prelude::IntoAttributeValue::into_attribute_value(source))
        .controls(true)
        .playsinline(true)
        .autoplay(true),
)
                                                    }
                                                },
                                            )
                                            .build();
                                        props
                                    },
                                )
                            },
                            leptos::html::div()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            move || {
                                                if current() {
                                                    selected
                                                        .get()
                                                        .map(|span| {
                                                            format!(
                                                                "{} · {}", date(& span.start), duration(span.duration)
                                                            )
                                                        })
                                                        .unwrap_or(t("尚未选择录像", "No recording selected"))
                                                } else {
                                                    t("尚未选择录像", "No recording selected")
                                                }
                                            }
                                        })
                                    },
                                ),
                        ))
                        .class("playback-stage"),
                ))
                .class("recording-layout xcss-content-panel"),
        ))
        .class("view active xcss-content-stack"),
)
}

#[component]
fn CameraSelect(cameras: Signal<Vec<Camera>>, selected: RwSignal<String>) -> impl IntoView {
    let open = RwSignal::new(false);
    View::new(
        leptos::html::div()
            .child((
                leptos::html::select()
                    .child(
                        #[allow(unused_braces)]
                        {
                            {
                                #[allow(unreachable_code)]
                                #[allow(unused_mut)]
                                #[allow(
                                    clippy::let_and_return,
                                    clippy::let_unit_value,
                                    clippy::unit_arg
                                )]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &For,
                                    {
                                        let mut props = component_props_builder(&For)
                                            .each(
                                                #[allow(unused_braces)]
                                                {
                                                    move || cameras.get()
                                                },
                                            )
                                            .key(
                                                #[allow(unused_braces)]
                                                {
                                                    |c| (c.id.clone(), c.name.clone())
                                                },
                                            )
                                            .children(
                                                #[allow(unused_braces)]
                                                {
                                                    move |c| {
                                                        View::new(
    leptos::html::option()
        .child(
            #[allow(unused_braces)]
            { IntoRender::into_render({ c.name }) },
        )
        .value(::leptos::prelude::IntoAttributeValue::into_attribute_value(c.id)),
)
                                                    }
                                                },
                                            )
                                            .build();
                                        props
                                    },
                                )
                            }
                        },
                    )
                    .class("xcss-select-native")
                    .tabindex("-1")
                    .aria_hidden("true")
                    .prop("value", move || selected.get())
                    .on(leptos::ev::change, move |event| {
                        selected.set(event_target_value(&event))
                    }),
                leptos::html::button()
                    .child((
                        leptos::html::span()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        move || {
                                            cameras
                                                .get()
                                                .into_iter()
                                                .find(|c| c.id == selected.get())
                                                .map(|c| c.name)
                                                .unwrap_or_default()
                                        }
                                    })
                                },
                            )
                            .class("xcss-select-value"),
                        leptos::svg::svg()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    leptos::svg::path()
                                        .attr("d", "m6 3 5 5-5 5")
                                        .attr("stroke", "currentColor")
                                },
                            )
                            .class("xcss-select-arrow")
                            .attr("aria-hidden", "true")
                            .attr("viewBox", "0 0 16 16"),
                    ))
                    .class("xcss-input xcss-select-trigger")
                    .r#type("button")
                    .role("combobox")
                    .aria_label(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        t("摄像头", "Camera"),
                    ))
                    .aria_haspopup("listbox")
                    .aria_controls("xcos-recording-camera-options")
                    .aria_expanded(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        move || open.get().to_string(),
                    ))
                    .on(leptos::ev::click, move |_| open.update(|v| *v = !*v))
                    .on(leptos::ev::keydown, move |event: web_sys::KeyboardEvent| {
                        let options = cameras.get_untracked();
                        let index = options
                            .iter()
                            .position(|c| c.id == selected.get_untracked())
                            .unwrap_or_default();
                        let next = match event.key().as_str() {
                            "ArrowDown" => Some((index + 1).min(options.len().saturating_sub(1))),
                            "ArrowUp" => Some(index.saturating_sub(1)),
                            "Home" => Some(0),
                            "End" => Some(options.len().saturating_sub(1)),
                            "Escape" => {
                                open.set(false);
                                event.prevent_default();
                                None
                            }
                            _ => None,
                        };
                        if let Some(index) = next {
                            event.prevent_default();
                            if let Some(camera) = options.get(index) {
                                selected.set(camera.id.clone())
                            }
                        }
                    }),
                leptos::html::div()
                    .child(
                        #[allow(unused_braces)]
                        {
                            {
                                #[allow(unreachable_code)]
                                #[allow(unused_mut)]
                                #[allow(
                                    clippy::let_and_return,
                                    clippy::let_unit_value,
                                    clippy::unit_arg
                                )]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &For,
                                    {
                                        let mut props = component_props_builder(&For)
                                            .each(
                                                #[allow(unused_braces)]
                                                {
                                                    move || cameras.get()
                                                },
                                            )
                                            .key(
                                                #[allow(unused_braces)]
                                                {
                                                    |c| (c.id.clone(), c.name.clone())
                                                },
                                            )
                                            .children(
                                                #[allow(unused_braces)]
                                                {
                                                    move |c| {
                                                        let id = c.id.clone();
                                                        let choice = c.id;
                                                        View::new(
    leptos::html::button()
        .child(
            #[allow(unused_braces)]
            { IntoRender::into_render({ c.name }) },
        )
        .class("xcss-select-option")
        .r#type("button")
        .role("option")
        .aria_selected(
            ::leptos::prelude::IntoAttributeValue::into_attribute_value(move || {
                (selected.get() == id).to_string()
            }),
        )
        .on(
            leptos::ev::click,
            move |_| {
                selected.set(choice.clone());
                open.set(false);
            },
        ),
)
                                                    }
                                                },
                                            )
                                            .build();
                                        props
                                    },
                                )
                            }
                        },
                    )
                    .class("xcss-select-panel")
                    .hidden(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        move || !open.get(),
                    ))
                    .id("xcos-recording-camera-options")
                    .role("listbox")
                    .aria_label(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        t("摄像头", "Camera"),
                    ))
                    .style(("position", "absolute"))
                    .style(("top", "100%"))
                    .style(("left", "0"))
                    .style(("right", "0"))
                    .style(("z-index", "10"))
                    .style(("max-height", "18rem")),
            ))
            .class("xcss-select")
            .style(("position", "relative")),
    )
}
