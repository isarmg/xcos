use crate::{AppState, Confirm, Failure, model::*};
use leptos::component::{component_props_builder, component_view};
use leptos::prelude::*;
use serde_json::{Value, json};
use wasm_bindgen_futures::spawn_local;
use xcss_web::{
    api::{self, ApiError, RequestOptions},
    i18n::t,
};

type Cursors = [Option<String>; 3];
#[derive(Clone, Copy)]
struct LogState {
    range: RwSignal<Option<(String, String)>>,
    draft_valid: RwSignal<bool>,
    cursors: RwSignal<Cursors>,
    history: RwSignal<[Vec<Option<String>>; 3]>,
    next: RwSignal<Cursors>,
    rows: RwSignal<[Vec<Value>; 3]>,
    loaded: RwSignal<bool>,
    busy: RwSignal<bool>,
    error: RwSignal<Option<ApiError>>,
    generation: RwSignal<u64>,
    alive: RwSignal<bool>,
    controller: StoredValue<Option<web_sys::AbortController>, LocalStorage>,
}
impl LogState {
    fn calendar(self) {
        self.error.set(None);
        spawn_local(async move {
            match request::<Value>("/logs/calendar", "GET", None).await {
                Ok(value) => {
                    if self.alive.try_get_untracked() != Some(true) {
                        return;
                    }
                    if let Some(date) = value
                        .get("today")
                        .and_then(Value::as_str)
                        .filter(|v| valid_iso_date(v))
                    {
                        self.range.set(Some((date.into(), date.into())));
                        self.load(false)
                    } else {
                        self.error.set(Some(contract_error()))
                    }
                }
                Err(e) => {
                    if self.alive.try_get_untracked() == Some(true) {
                        self.error.set(Some(e))
                    }
                }
            }
        });
    }
    fn load(self, preserve: bool) {
        let Some((start, end)) = self.range.get_untracked() else {
            return;
        };
        self.generation.update(|g| *g += 1);
        let generation = self.generation.get_untracked();
        self.controller.update_value(|c| {
            if let Some(c) = c.take() {
                c.abort()
            }
        });
        if !preserve {
            self.loaded.set(false)
        }
        self.busy.set(true);
        self.error.set(None);
        let controller = web_sys::AbortController::new().unwrap();
        self.controller.set_value(Some(controller.clone()));
        let cursors = self.cursors.get_untracked();
        let query = if start == end {
            format!("date={start}")
        } else {
            format!("start_date={start}&end_date={end}")
        };
        spawn_local(async move {
            let path = |index: usize, endpoint: &str| {
                format!(
                    "/api/v1{endpoint}?{query}{}",
                    cursors[index]
                        .as_ref()
                        .map(|c| format!("&cursor={}", encode(c)))
                        .unwrap_or_default()
                )
            };
            let fetch = |path: String, index: usize| {
                let signal = controller.signal();
                async move {
                    let value = api::request_with_options(
                        &path,
                        "GET",
                        None,
                        RequestOptions {
                            signal: Some(signal),
                            timeout_ms: 10_000,
                            max_response_bytes: 8 * 1024 * 1024,
                            ..Default::default()
                        },
                    )
                    .await?;
                    let page: LogPage =
                        serde_json::from_value(value).map_err(|_| contract_error())?;
                    if page.format != "xcos-history-page-v1"
                        || page.items.len() > 100
                        || page.items.iter().any(|value| !valid_log_row(value, index))
                        || page.next_cursor.as_ref().is_some_and(|c| c.len() > 512)
                    {
                        return Err(contract_error());
                    }
                    Ok::<_, ApiError>(page)
                }
            };
            let (events, operations, audit) = futures::join!(
                fetch(path(0, "/events/logs"), 0),
                fetch(path(1, "/media/operations"), 1),
                fetch(path(2, "/audit"), 2)
            );
            if self.alive.try_get_untracked() != Some(true)
                || self.generation.try_get_untracked() != Some(generation)
            {
                return;
            }
            self.busy.set(false);
            match (events, operations, audit) {
                (Ok(events), Ok(operations), Ok(audit)) => {
                    self.rows.set([events.items, operations.items, audit.items]);
                    self.next.set([
                        events.next_cursor,
                        operations.next_cursor,
                        audit.next_cursor,
                    ]);
                    self.loaded.set(true)
                }
                (a, b, c) => {
                    self.loaded.set(false);
                    self.error.set(a.err().or(b.err()).or(c.err()));
                }
            }
        });
    }
    fn navigate(self, index: usize, step: &str) {
        if !self.loaded.get_untracked() || !self.draft_valid.get_untracked() {
            return;
        }
        let mut cursors = self.cursors.get_untracked();
        let mut history = self.history.get_untracked();
        match step {
            "next" => {
                let Some(next) = self.next.get_untracked()[index].clone() else {
                    return;
                };
                history[index].push(cursors[index].clone());
                cursors[index] = Some(next)
            }
            "previous" => {
                let Some(previous) = history[index].pop() else {
                    return;
                };
                cursors[index] = previous
            }
            _ => {
                cursors[index] = None;
                history[index].clear();
            }
        }
        self.cursors.set(cursors);
        self.history.set(history);
        self.load(false);
    }
}

#[component]
pub fn Logs() -> impl IntoView {
    let app = expect_context::<AppState>();
    let state = LogState {
        range: RwSignal::new(None),
        draft_valid: RwSignal::new(false),
        cursors: RwSignal::new([None, None, None]),
        history: RwSignal::new([vec![], vec![], vec![]]),
        next: RwSignal::new([None, None, None]),
        rows: RwSignal::new([vec![], vec![], vec![]]),
        loaded: RwSignal::new(false),
        busy: RwSignal::new(false),
        error: RwSignal::new(None),
        generation: RwSignal::new(0),
        alive: RwSignal::new(true),
        controller: StoredValue::new_local(None),
    };
    state.calendar();
    let last = RwSignal::new(app.logs_refresh.get_untracked());
    Effect::new(move |_| {
        let next = app.logs_refresh.get();
        if next != last.get_untracked() {
            last.set(next);
            if state.range.get_untracked().is_some() && state.error.get_untracked().is_none() {
                state.load(true)
            }
        }
    });
    on_cleanup(move || {
        state.alive.set(false);
        state.controller.update_value(|c| {
            if let Some(c) = c.take() {
                c.abort()
            }
        });
    });
    View::new(
    leptos::html::section()
        .child((
            leptos::html::section()
                .child((
                    leptos::html::p()
                        .child(
                            #[allow(unused_braces)]
                            {
                                IntoRender::into_render({
                                    t("实例：全部实例", "Instance: All instances")
                                })
                            },
                        ),
                    leptos::html::div()
                        .child((
                            leptos::html::label()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        IntoRender::into_render({
                                            t("日志日期范围（UTC）", "Log date range (UTC)")
                                        })
                                    },
                                )
                                .r#for("xcos-log-date-start-year"),
                            leptos::html::div()
                                .child(
                                    #[allow(unused_braces)]
                                    {
                                        leptos::html::button()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        t("刷新日志", "Refresh logs")
                                                    })
                                                },
                                            )
                                            .class("xcss-button")
                                            .disabled(
                                                ::leptos::prelude::IntoAttributeValue::into_attribute_value(move ||
                                                {
                                                    state.range.get().is_none() || !state.draft_valid.get()
                                                        || state.busy.get() && !state.loaded.get()
                                                }),
                                            )
                                            .on(
                                                leptos::ev::click,
                                                move |_| state.load(false),
                                            )
                                    },
                                )
                                .class("xcss-actions"),
                            {
                                #[allow(unreachable_code)] #[allow(unused_mut)]
                                #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                component_view(
                                    #[allow(clippy::needless_borrows_for_generic_args)]
                                    &DateRange,
                                    {
                                        let mut props = component_props_builder(
                                                &DateRange,
                                            )
                                            .range(#[allow(unused_braces)] { state.range })
                                            .valid(#[allow(unused_braces)] { state.draft_valid })
                                            .apply(
                                                #[allow(unused_braces)]
                                                {
                                                    Callback::new(move |range| {
                                                        state.range.set(Some(range));
                                                        state.cursors.set([None, None, None]);
                                                        state.history.set([vec![], vec![], vec![]]);
                                                        state.load(false);
                                                    })
                                                },
                                            )
                                            .build();
                                        props
                                    },
                                )
                            },
                        ))
                        .class("xcss-log-date-controls"),
                ))
                .class("xcss-content-panel xcss-content-stack")
                .aria_label(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        t("日志筛选", "Log filters"),
                    ),
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
                                { move || state.error.get().is_some() },
                            )
                            .children({
                                ::leptos::children::ToChildren::to_children(move || {
                                    leptos::html::div()
                                        .child((
                                            leptos::html::p()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            move || state.error.get().map(|error| error.to_string())
                                                        })
                                                    },
                                                ),
                                            leptos::html::button()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            t("重试", "Retry")
                                                        })
                                                    },
                                                )
                                                .class("xcss-button")
                                                .on(
                                                    leptos::ev::click,
                                                    move |_| {
                                                        if state.range.get_untracked().is_none() {
                                                            state.calendar()
                                                        } else {
                                                            state.load(false)
                                                        }
                                                    },
                                                ),
                                        ))
                                        .class("xcss-error")
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
                    &Show,
                    {
                        let mut props = component_props_builder(
                                &Show,
                            )
                            .when(
                                #[allow(unused_braces)]
                                {
                                    move || !state.loaded.get() && state.error.get().is_none()
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
                                                        "正在加载所选日期的日志…",
                                                        "Loading logs for the selected date…",
                                                    )
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
            leptos::html::section()
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
                                        { move || state.loaded.get() },
                                    )
                                    .children({
                                        ::leptos::children::ToChildren::to_children(move || {
                                            #[allow(unreachable_code)] #[allow(unused_mut)]
                                            #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                            component_view(
                                                #[allow(clippy::needless_borrows_for_generic_args)]
                                                &Events,
                                                {
                                                    let mut props = component_props_builder(
                                                            &Events,
                                                        )
                                                        .rows(
                                                            #[allow(unused_braces)]
                                                            {
                                                                Signal::derive(move || {
                                                                    state.rows.with(|rows| rows[0].clone())
                                                                })
                                                            },
                                                        )
                                                        .acknowledge(
                                                            #[allow(unused_braces)]
                                                            {
                                                                Callback::new(move |id: String| {
                                                                    spawn_local(async move {
                                                                        match command(&format!("/events/{id}/ack"), "POST", None)
                                                                            .await
                                                                        {
                                                                            Ok(()) => state.load(true),
                                                                            Err(e) => state.error.set(Some(e)),
                                                                        }
                                                                    });
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
                            &Pager,
                            {
                                let mut props = component_props_builder(
                                        &Pager,
                                    )
                                    .state(#[allow(unused_braces)] { state })
                                    .index(#[allow(unused_braces)] { 0 })
                                    .label(
                                        #[allow(unused_braces)]
                                        { t("监控事件分页", "Event history pages") },
                                    )
                                    .build();
                                props
                            },
                        )
                    },
                ))
                .class("xcss-content-stack")
                .aria_label(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        t("监控事件", "Monitoring events"),
                    ),
                ),
            leptos::html::section()
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
                                        { move || state.loaded.get() },
                                    )
                                    .children({
                                        ::leptos::children::ToChildren::to_children(move || {
                                            #[allow(unreachable_code)] #[allow(unused_mut)]
                                            #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                            component_view(
                                                #[allow(clippy::needless_borrows_for_generic_args)]
                                                &Operations,
                                                {
                                                    let mut props = component_props_builder(
                                                            &Operations,
                                                        )
                                                        .rows(
                                                            #[allow(unused_braces)]
                                                            {
                                                                Signal::derive(move || {
                                                                    state.rows.with(|rows| rows[1].clone())
                                                                })
                                                            },
                                                        )
                                                        .changed(
                                                            #[allow(unused_braces)]
                                                            { Callback::new(move |_| state.load(true)) },
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
                            &Pager,
                            {
                                let mut props = component_props_builder(
                                        &Pager,
                                    )
                                    .state(#[allow(unused_braces)] { state })
                                    .index(#[allow(unused_braces)] { 1 })
                                    .label(
                                        #[allow(unused_braces)]
                                        { t("媒体操作分页", "Media operation pages") },
                                    )
                                    .build();
                                props
                            },
                        )
                    },
                ))
                .class("xcss-content-stack")
                .aria_label(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        t("媒体操作日志", "Media operation logs"),
                    ),
                ),
            leptos::html::section()
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
                                        { move || state.loaded.get() },
                                    )
                                    .children({
                                        ::leptos::children::ToChildren::to_children(move || {
                                            #[allow(unreachable_code)] #[allow(unused_mut)]
                                            #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
                                            component_view(
                                                #[allow(clippy::needless_borrows_for_generic_args)]
                                                &Audit,
                                                {
                                                    let mut props = component_props_builder(
                                                            &Audit,
                                                        )
                                                        .rows(
                                                            #[allow(unused_braces)]
                                                            {
                                                                Signal::derive(move || {
                                                                    state.rows.with(|rows| rows[2].clone())
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
                            &Pager,
                            {
                                let mut props = component_props_builder(
                                        &Pager,
                                    )
                                    .state(#[allow(unused_braces)] { state })
                                    .index(#[allow(unused_braces)] { 2 })
                                    .label(
                                        #[allow(unused_braces)]
                                        { t("审计日志分页", "Audit history pages") },
                                    )
                                    .build();
                                props
                            },
                        )
                    },
                ))
                .class("xcss-content-stack")
                .aria_label(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        t("审计日志", "Audit logs"),
                    ),
                ),
        ))
        .class("xcss-content-stack")
        .aria_label(
            ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                t("日志", "Logs"),
            ),
        ),
)
}
#[component]
fn Pager(state: LogState, index: usize, label: String) -> impl IntoView {
    View::new(
        leptos::html::nav()
            .child((
                leptos::html::button()
                    .child(
                        #[allow(unused_braces)]
                        {
                            IntoRender::into_render({ t("首页", "First page") })
                        },
                    )
                    .class("xcss-button")
                    .disabled(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        move || {
                            !state.loaded.get()
                                || !state.draft_valid.get()
                                || state.cursors.get()[index].is_none()
                        },
                    ))
                    .on(leptos::ev::click, move |_| state.navigate(index, "first")),
                leptos::html::button()
                    .child(
                        #[allow(unused_braces)]
                        {
                            IntoRender::into_render({ t("上一页", "Previous page") })
                        },
                    )
                    .class("xcss-button")
                    .disabled(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        move || {
                            !state.loaded.get()
                                || !state.draft_valid.get()
                                || state.history.get()[index].is_empty()
                        },
                    ))
                    .on(leptos::ev::click, move |_| {
                        state.navigate(index, "previous")
                    }),
                leptos::html::button()
                    .child(
                        #[allow(unused_braces)]
                        {
                            IntoRender::into_render({ t("下一页", "Next page") })
                        },
                    )
                    .class("xcss-button")
                    .disabled(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        move || {
                            !state.loaded.get()
                                || !state.draft_valid.get()
                                || state.next.get()[index].is_none()
                        },
                    ))
                    .on(leptos::ev::click, move |_| state.navigate(index, "next")),
            ))
            .class("xcss-actions")
            .aria_label(::leptos::prelude::IntoAttributeValue::into_attribute_value(
                label,
            )),
    )
}

#[component]
fn Events(rows: Signal<Vec<Value>>, acknowledge: Callback<String>) -> impl IntoView {
    let app = expect_context::<AppState>();
    View::new({
        #[allow(unreachable_code)]
        #[allow(unused_mut)]
        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
        component_view(
            #[allow(clippy::needless_borrows_for_generic_args)]
            &Show,
            {
                let mut props = component_props_builder(&Show)
                .when(#[allow(unused_braces)] { move || !rows.get().is_empty() })
                .fallback(
                    #[allow(unused_braces)]
                    {
                        move || {
                            View::new(
    leptos::html::p()
        .child(
            #[allow(unused_braces)]
            {
                IntoRender::into_render({
                    t("没有事件", "No events")
                })
            },
        )
        .class("empty-state"),
)
                        }
                    },
                )
                .children({
                    ::leptos::children::ToChildren::to_children(move || {
                        leptos::html::div()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    leptos::html::table()
                                        .child((
                                            leptos::html::caption()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            t("监控事件", "Monitoring events")
                                                        })
                                                    },
                                                ),
                                            leptos::html::thead()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        leptos::html::tr()
                                                            .child(
                                                                #[allow(unused_braces)]
                                                                {
                                                                    IntoRender::into_render({
                                                                        [
                                                                            ("服务器时间", "Server time"),
                                                                            ("等级", "Severity"),
                                                                            ("事件", "Event"),
                                                                            ("摄像头", "Camera"),
                                                                            ("状态", "Status"),
                                                                        ]
                                                                            .into_iter()
                                                                            .map(|(zh, en)| {
                                                                                View::new(
    leptos::html::th()
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
                                            leptos::html::tbody()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
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
                                                                        .each(#[allow(unused_braces)] { move || rows.get() })
                                                                        .key(
                                                                            #[allow(unused_braces)]
                                                                            {
                                                                                |v| {
                                                                                    format!(
                                                                                        "{}:{}", value(v, "id"), value(v, "acknowledged_at")
                                                                                    )
                                                                                }
                                                                            },
                                                                        )
                                                                        .children(
                                                                            #[allow(unused_braces)]
                                                                            {
                                                                                move |row| {
                                                                                    let id = value(&row, "id");
                                                                                    let camera = value(&row, "camera_id");
                                                                                    View::new(
    leptos::html::tr()
        .child((
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        leptos::html::time()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        value(&row, "created_at")
                                    })
                                },
                            )
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            match value(&row, "severity").as_str() {
                                "info" => t("信息", "Information"),
                                "warning" => t("警告", "Warning"),
                                "critical" => t("严重", "Critical"),
                                _ => t("未知", "Unknown"),
                            }
                        })
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            label(&value(&row, "kind"))
                        })
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || {
                                app
                                    .cameras
                                    .get()
                                    .into_iter()
                                    .find(|c| c.id == camera)
                                    .map(|c| c.name)
                                    .unwrap_or(t("系统", "System"))
                            }
                        })
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            if row.get("acknowledged_at").is_none_or(Value::is_null) {
                                View::new(
    leptos::html::div()
        .child(
            #[allow(unused_braces)]
            {
                leptos::html::button()
                    .child(
                        #[allow(unused_braces)]
                        {
                            IntoRender::into_render({
                                t("确认", "Acknowledge")
                            })
                        },
                    )
                    .class("xcss-button")
                    .on(
                        leptos::ev::click,
                        move |_| acknowledge.run(id.clone()),
                    )
            },
        )
        .class("xcss-actions"),
)
                                    .into_any()
                            } else {
                                t("已确认", "Acknowledged").into_any()
                            }
                        })
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
                                                        }
                                                    },
                                                ),
                                        ))
                                        .class("xcss-table xcos-log-table")
                                        .aria_label(
                                            ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                                                t("监控事件", "Monitoring events"),
                                            ),
                                        )
                                },
                            )
                            .class("xcss-table-scroll")
                    })
                })
                .build();
                props
            },
        )
    })
}
#[component]
fn Audit(rows: Signal<Vec<Value>>) -> impl IntoView {
    View::new({
        #[allow(unreachable_code)]
        #[allow(unused_mut)]
        #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
        component_view(
            #[allow(clippy::needless_borrows_for_generic_args)]
            &Show,
            {
                let mut props = component_props_builder(&Show)
                .when(#[allow(unused_braces)] { move || !rows.get().is_empty() })
                .fallback(
                    #[allow(unused_braces)]
                    {
                        move || {
                            View::new(
    leptos::html::p()
        .child(
            #[allow(unused_braces)]
            {
                IntoRender::into_render({
                    t("暂无审计日志", "No audit logs yet")
                })
            },
        )
        .class("empty-state"),
)
                        }
                    },
                )
                .children({
                    ::leptos::children::ToChildren::to_children(move || {
                        leptos::html::div()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    leptos::html::table()
                                        .child((
                                            leptos::html::caption()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        IntoRender::into_render({
                                                            t("审计日志", "Audit logs")
                                                        })
                                                    },
                                                ),
                                            leptos::html::thead()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
                                                        leptos::html::tr()
                                                            .child(
                                                                #[allow(unused_braces)]
                                                                {
                                                                    IntoRender::into_render({
                                                                        [
                                                                            ("服务器时间", "Server time"),
                                                                            ("操作", "Action"),
                                                                            ("操作者", "Actor"),
                                                                            ("对象", "Entity"),
                                                                            ("详情", "Details"),
                                                                        ]
                                                                            .into_iter()
                                                                            .map(|(zh, en)| {
                                                                                View::new(
    leptos::html::th()
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
                                            leptos::html::tbody()
                                                .child(
                                                    #[allow(unused_braces)]
                                                    {
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
                                                                        .each(#[allow(unused_braces)] { move || rows.get() })
                                                                        .key(#[allow(unused_braces)] { |v| value(v, "id") })
                                                                        .children(
                                                                            #[allow(unused_braces)]
                                                                            {
                                                                                move |row| {
                                                                                    View::new(
    leptos::html::tr()
        .child((
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        leptos::html::time()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        value(&row, "created_at")
                                    })
                                },
                            )
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            label(&value(&row, "action"))
                        })
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        leptos::html::code()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        row.get("user_id")
                                            .and_then(Value::as_str)
                                            .map(String::from)
                                            .unwrap_or(t("系统", "System"))
                                    })
                                },
                            )
                    },
                ),
            leptos::html::td()
                .child((
                    IntoRender::into_render({
                        label(&value(&row, "entity_type"))
                    }),
                    IntoRender::into_render({
                        row.get("entity_id")
                            .and_then(Value::as_str)
                            .map(|id| {
                                View::new((
    " / ",
    leptos::html::code()
        .child(
            #[allow(unused_braces)]
            { IntoRender::into_render({ id.to_string() }) },
        ),
))
                            })
                    }),
                )),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        leptos::html::details()
                            .child((
                                leptos::html::summary()
                                    .child(
                                        #[allow(unused_braces)]
                                        {
                                            IntoRender::into_render({
                                                t("详情", "Details")
                                            })
                                        },
                                    ),
                                leptos::html::code()
                                    .child(
                                        #[allow(unused_braces)]
                                        {
                                            IntoRender::into_render({
                                                safe_details(row.get("details").unwrap_or(&Value::Null))
                                            })
                                        },
                                    ),
                            ))
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
                                                        }
                                                    },
                                                ),
                                        ))
                                        .class("xcss-table xcos-log-table")
                                        .aria_label(
                                            ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                                                t("审计日志", "Audit logs"),
                                            ),
                                        )
                                },
                            )
                            .class("xcss-table-scroll")
                    })
                })
                .build();
                props
            },
        )
    })
}

#[component]
fn Operations(rows: Signal<Vec<Value>>, changed: Callback<()>) -> impl IntoView {
    let app = expect_context::<AppState>();
    let pending = RwSignal::new(false);
    let error = RwSignal::new(None);
    let resolution = RwSignal::new(None::<(String, String, String)>);
    View::new(
    leptos::html::section()
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
                                { move || !rows.get().is_empty() },
                            )
                            .fallback(
                                #[allow(unused_braces)]
                                {
                                    move || {
                                        View::new(
    leptos::html::p()
        .child(
            #[allow(unused_braces)]
            {
                IntoRender::into_render({
                    t(
                        "暂无媒体协调操作",
                        "No media reconciliation operations",
                    )
                })
            },
        )
        .class("empty-state"),
)
                                    }
                                },
                            )
                            .children({
                                ::leptos::children::ToChildren::to_children(move || {
                                    leptos::html::div()
                                        .child(
                                            #[allow(unused_braces)]
                                            {
                                                leptos::html::table()
                                                    .child((
                                                        leptos::html::caption()
                                                            .child(
                                                                #[allow(unused_braces)]
                                                                {
                                                                    IntoRender::into_render({
                                                                        t("媒体协调操作", "Media reconciliation operations")
                                                                    })
                                                                },
                                                            ),
                                                        leptos::html::thead()
                                                            .child(
                                                                #[allow(unused_braces)]
                                                                {
                                                                    leptos::html::tr()
                                                                        .child(
                                                                            #[allow(unused_braces)]
                                                                            {
                                                                                IntoRender::into_render({
                                                                                    [
                                                                                        ("服务器时间", "Server time"),
                                                                                        ("摄像头", "Camera"),
                                                                                        ("状态", "Status"),
                                                                                        ("详情", "Details"),
                                                                                    ]
                                                                                        .into_iter()
                                                                                        .map(|(zh, en)| {
                                                                                            View::new(
    leptos::html::th()
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
                                                        leptos::html::tbody()
                                                            .child(
                                                                #[allow(unused_braces)]
                                                                {
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
                                                                                    .each(#[allow(unused_braces)] { move || rows.get() })
                                                                                    .key(
                                                                                        #[allow(unused_braces)]
                                                                                        {
                                                                                            |v| {
                                                                                                format!(
                                                                                                    "{}:{}:{}", value(v, "id"), value(v, "state"), v
                                                                                                    .get("attempt").unwrap_or(& Value::Null)
                                                                                                )
                                                                                            }
                                                                                        },
                                                                                    )
                                                                                    .children(
                                                                                        #[allow(unused_braces)]
                                                                                        {
                                                                                            move |row| {
                                                                                                let camera = value(&row, "camera_id");
                                                                                                let id = value(&row, "id");
                                                                                                let state = value(&row, "state");
                                                                                                View::new(
    leptos::html::tr()
        .child((
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        leptos::html::time()
                            .child(
                                #[allow(unused_braces)]
                                {
                                    IntoRender::into_render({
                                        value(&row, "created_at")
                                    })
                                },
                            )
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            move || {
                                app
                                    .cameras
                                    .get()
                                    .into_iter()
                                    .find(|c| c.id == camera)
                                    .map(|c| c.name)
                                    .unwrap_or(camera.clone())
                            }
                        })
                    },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    { IntoRender::into_render({ label(&state) }) },
                ),
            leptos::html::td()
                .child(
                    #[allow(unused_braces)]
                    {
                        leptos::html::details()
                            .child((
                                leptos::html::summary()
                                    .child(
                                        #[allow(unused_braces)]
                                        {
                                            IntoRender::into_render({
                                                t("详情", "Details")
                                            })
                                        },
                                    ),
                                leptos::html::dl()
                                    .child((
                                        leptos::html::dt()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        t("操作标识", "Operation ID")
                                                    })
                                                },
                                            ),
                                        leptos::html::dd()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    leptos::html::code()
                                                        .child(
                                                            #[allow(unused_braces)]
                                                            {
                                                                IntoRender::into_render({ id.clone() })
                                                            },
                                                        )
                                                },
                                            ),
                                        leptos::html::dt()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        t("原因", "Reason")
                                                    })
                                                },
                                            ),
                                        leptos::html::dd()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        label(&value(&row, "reason"))
                                                    })
                                                },
                                            ),
                                        leptos::html::dt()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        t("代际 / 尝试", "Generation / attempts")
                                                    })
                                                },
                                            ),
                                        leptos::html::dd()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        format!(
                                                            "{} / {} of {}", row.get("generation").unwrap_or(&
                                                            Value::Null), row.get("attempt").unwrap_or(& Value::Null),
                                                            row.get("max_attempts").unwrap_or(& Value::Null)
                                                        )
                                                    })
                                                },
                                            ),
                                        leptos::html::dt()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        t("错误", "Error")
                                                    })
                                                },
                                            ),
                                        leptos::html::dd()
                                            .child(
                                                #[allow(unused_braces)]
                                                {
                                                    IntoRender::into_render({
                                                        value(&row, "error_code")
                                                    })
                                                },
                                            ),
                                    ))
                                    .class("xcos-detail-list"),
                                IntoRender::into_render({
                                    if ["unknown", "failed", "dead_letter"]
                                        .contains(&state.as_str())
                                    {
                                        [
                                            (
                                                "confirmed_succeeded",
                                                "确认已成功",
                                                "Confirm success",
                                            ),
                                            ("confirmed_failed", "确认未成功", "Confirm failure"),
                                            (
                                                "unable_to_confirm",
                                                "仍无法确认",
                                                "Still unable to confirm",
                                            ),
                                        ]
                                            .into_iter()
                                            .map(|(choice, zh, en)| {
                                                let id = id.clone();
                                                View::new(
    leptos::html::button()
        .child(
            #[allow(unused_braces)]
            { IntoRender::into_render({ t(zh, en) }) },
        )
        .class("xcss-button")
        .disabled(
            ::leptos::prelude::IntoAttributeValue::into_attribute_value(move || {
                pending.get()
            }),
        )
        .on(
            leptos::ev::click,
            move |_| resolution.set(Some((id.clone(), choice.into(), t(zh, en)))),
        ),
)
                                            })
                                            .collect_view()
                                            .into_any()
                                    } else {
                                        ().into_any()
                                    }
                                }),
                            ))
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
                                                                    }
                                                                },
                                                            ),
                                                    ))
                                                    .class("xcss-table xcos-log-table")
                                                    .aria_label(
                                                        ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                                                            t("媒体协调操作", "Media reconciliation operations"),
                                                        ),
                                                    )
                                            },
                                        )
                                        .class("xcss-table-scroll")
                                })
                            })
                            .build();
                        props
                    },
                )
            },
            IntoRender::into_render({
                move || {
                    resolution
                        .get()
                        .map(|(id, choice, title)| {
                            View::new({
    #[allow(unreachable_code)] #[allow(unused_mut)] #[allow(clippy::let_and_return, clippy::let_unit_value, clippy::unit_arg)]
    component_view(
        #[allow(clippy::needless_borrows_for_generic_args)]
        &Confirm,
        {
            let mut props = component_props_builder(&Confirm)
                .title(#[allow(unused_braces)] { title })
                .description(
                    #[allow(unused_braces)]
                    {
                        t(
                            "请先核对 MediaMTX 与摄像头的实际状态。此操作只记录人工结论，不会重新执行原媒体操作。",
                            "Check the actual MediaMTX and camera state first. This only records a manual conclusion and does not rerun the media operation.",
                        )
                    },
                )
                .close(
                    #[allow(unused_braces)]
                    { Callback::new(move |_| resolution.set(None)) },
                )
                .accept(
                    #[allow(unused_braces)]
                    {
                        Callback::new(move |_| {
                            if pending.get_untracked() {
                                return;
                            }
                            pending.set(true);
                            resolution.set(None);
                            let id = id.clone();
                            let choice = choice.clone();
                            spawn_local(async move {
                                let result = command(
                                        &format!("/media/operations/{id}/resolve"),
                                        "POST",
                                        Some(json!({ "resolution" : choice })),
                                    )
                                    .await;
                                if pending.is_disposed() {
                                    return;
                                }
                                pending.set(false);
                                match result {
                                    Ok(()) => {
                                        changed.run(());
                                        app.toast(
                                            t(
                                                "人工结论已记录；原媒体操作不会重新执行",
                                                "The manual conclusion was recorded; the media operation was not rerun",
                                            ),
                                        )
                                    }
                                    Err(e) => error.set(Some(e)),
                                }
                            });
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
        .class("xcss-content-stack")
        .aria_label(
            ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                t("媒体协调操作", "Media reconciliation operations"),
            ),
        ),
)
}

#[derive(Clone, Debug, PartialEq)]
struct Validation {
    range: Option<(String, String)>,
    invalid: [bool; 6],
    reversed: bool,
}
fn validate(draft: &[String; 6]) -> Validation {
    let mut invalid = [false; 6];
    let mut dates = [String::new(), String::new()];
    for (endpoint, date) in dates.iter_mut().enumerate() {
        let offset = endpoint * 3;
        let parts: [Option<u32>; 3] = std::array::from_fn(|i| {
            let value = &draft[offset + i];
            if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
                None
            } else {
                value.parse::<u32>().ok()
            }
        });
        for i in 0..3 {
            invalid[offset + i] = parts[i].is_none();
        }
        if let [Some(year), Some(month), Some(day)] = parts {
            if !(1..=9999).contains(&year) {
                invalid[offset] = true
            }
            if !(1..=12).contains(&month) {
                invalid[offset + 1] = true
            }
            let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
            let max = match month {
                2 => {
                    if leap {
                        29
                    } else {
                        28
                    }
                }
                4 | 6 | 9 | 11 => 30,
                _ => 31,
            };
            if day < 1 || day > max {
                invalid[offset + 2] = true
            }
            *date = format!("{year:04}-{month:02}-{day:02}");
        }
    }
    let reversed = !invalid.iter().any(|v| *v) && dates[0] > dates[1];
    if reversed {
        invalid = [true; 6]
    }
    Validation {
        range: if invalid.iter().any(|v| *v) {
            None
        } else {
            Some((dates[0].clone(), dates[1].clone()))
        },
        invalid,
        reversed,
    }
}
fn valid_iso_date(value: &str) -> bool {
    let parts: Vec<_> = value.split('-').map(String::from).collect();
    value.len() == 10
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
        && parts.len() == 3
        && validate(&[
            parts[0].clone(),
            parts[1].clone(),
            parts[2].clone(),
            parts[0].clone(),
            parts[1].clone(),
            parts[2].clone(),
        ])
        .range
        .is_some()
}
#[component]
fn DateRange(
    range: RwSignal<Option<(String, String)>>,
    valid: RwSignal<bool>,
    apply: Callback<(String, String)>,
) -> impl IntoView {
    let draft = RwSignal::new(std::array::from_fn::<String, 6, _>(|_| String::new()));
    let loaded = RwSignal::new(None::<(String, String)>);
    Effect::new(move |_| {
        if let Some(range) = range.get()
            && loaded.get_untracked().as_ref() != Some(&range)
        {
            let mut parts = range
                .0
                .split('-')
                .chain(range.1.split('-'))
                .map(String::from)
                .collect::<Vec<_>>();
            for index in [1, 2, 4, 5] {
                parts[index] = parts[index]
                    .parse::<u32>()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
            }
            draft.set(parts.try_into().unwrap());
            loaded.set(Some(range));
        }
    });
    let validation = Memo::new(move |_| validate(&draft.get()));
    Effect::new(move |_| valid.set(validation.get().range.is_some()));
    View::new(
    leptos::html::div()
        .child((
            leptos::html::div()
                .child(
                    #[allow(unused_braces)]
                    {
                        IntoRender::into_render({
                            (0..2)
                                .map(move |endpoint| {
                                    View::new(
    leptos::html::span()
        .child((
            IntoRender::into_render({
                (0..3)
                    .map(move |part| {
                        let index = endpoint * 3 + part;
                        let id = format!(
                            "xcos-log-date-{}-{}", if endpoint == 0 { "start" } else {
                            "end" }, ["year", "month", "day"] [part]
                        );
                        View::new(
    leptos::html::span()
        .child((
            leptos::html::input()
                .class("xcss-date-range-number")
                .id(::leptos::prelude::IntoAttributeValue::into_attribute_value(id))
                .r#type("text")
                .inputmode("numeric")
                .size(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        if part == 0 { 4 } else { 2 },
                    ),
                )
                .maxlength(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        if part == 0 { 4 } else { 2 },
                    ),
                )
                .aria_label(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(
                        format!(
                            "{} {}", if endpoint == 0 { t("开始", "Start") } else {
                            t("结束", "End") }, [t("年", "year"), t("月", "month"),
                            t("日", "day")] [part]
                        ),
                    ),
                )
                .aria_invalid(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(move ||
                    {
                        if range.get().is_some() && validation.get().invalid[index] {
                            Some("true")
                        } else {
                            None
                        }
                    }),
                )
                .disabled(
                    ::leptos::prelude::IntoAttributeValue::into_attribute_value(move ||
                    range.get().is_none()),
                )
                .prop("value", move || draft.get()[index].clone())
                .on(
                    leptos::ev::input,
                    move |event| {
                        draft.update(|v| v[index] = event_target_value(&event))
                    },
                )
                .on(
                    leptos::ev::keydown,
                    move |event: web_sys::KeyboardEvent| {
                        if event.key() == "Enter" && !event.is_composing() {
                            event.prevent_default();
                            if let Some(range) = validation.get_untracked().range {
                                apply.run(range);
                            }
                        }
                    },
                ),
            IntoRender::into_render({
                (part < 2)
                    .then(|| {
                        View::new(
    leptos::html::span()
        .child(#[allow(unused_braces)] { "/" })
        .class("xcss-date-range-separator"),
)
                    })
            }),
        ))
        .class("xcss-date-range-part"),
)
                    })
                    .collect_view()
            }),
            IntoRender::into_render({
                (endpoint == 0)
                    .then(|| {
                        View::new(
    leptos::html::span()
        .child(#[allow(unused_braces)] { "–" })
        .class("xcss-date-range-separator"),
)
                    })
            }),
        ))
        .class("xcss-date-range-endpoint"),
)
                                })
                                .collect_view()
                        })
                    },
                )
                .class("xcss-date-range-fields"),
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
                                        range.get().is_some() && validation.get().range.is_none()
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
                                                    move || {
                                                        if validation.get().reversed {
                                                            t(
                                                                "结束日期不能早于开始日期。",
                                                                "The end date cannot be earlier than the start date.",
                                                            )
                                                        } else {
                                                            t(
                                                                "请输入有效的年月日。",
                                                                "Enter a valid year, month, and day.",
                                                            )
                                                        }
                                                    }
                                                })
                                            },
                                        )
                                        .class("xcss-date-range-error")
                                        .role("alert")
                                })
                            })
                            .build();
                        props
                    },
                )
            },
        ))
        .class("xcss-date-range")
        .id("xcos-log-date"),
)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn draft(start: &str, end: &str) -> [String; 6] {
        start
            .split('-')
            .chain(end.split('-'))
            .map(String::from)
            .collect::<Vec<_>>()
            .try_into()
            .unwrap()
    }
    #[test]
    fn leap_dates_and_invalid_parts() {
        assert!(validate(&draft("2024-02-29", "2024-02-29")).range.is_some());
        let v = validate(&draft("2022-02-30", "2023-02-02"));
        assert_eq!(v.invalid, [false, false, true, false, false, false]);
        assert!(validate(&draft("0000-01-01", "2025-01-01")).range.is_none());
        assert!(validate(&draft("2024-13-01", "2025-01-01")).invalid[1]);
    }
    #[test]
    fn reversed_range_marks_every_part() {
        let v = validate(&draft("2023-02-02", "2022-02-01"));
        assert_eq!(v.invalid, [true; 6]);
        assert!(v.reversed);
    }
}
