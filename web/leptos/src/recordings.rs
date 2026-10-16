use crate::{Failure, model::*};
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
    view! {<section class="view active xcss-content-stack"><div class="filter-panel xcss-content-panel"><label>{t("摄像头","Camera")}<CameraSelect cameras=cameras selected=camera/></label><label>{t("开始时间（UTC）","Start time (UTC)")}<input class="xcss-input" type="datetime-local" prop:value=move||start.get() on:input=move|e|start.set(event_target_value(&e))/></label><label>{t("结束时间（UTC）","End time (UTC)")}<input class="xcss-input" type="datetime-local" prop:value=move||end.get() on:input=move|e|end.set(event_target_value(&e))/></label><button class="xcss-button button button-primary" on:click=move |_|search()>{t("查询录像","Search recordings")}</button></div>
        <div class="recording-layout xcss-content-panel"><div><div class="section-heading"><h3>{t("录像时间段","Recording segments")}</h3><span>{move||if current(){spans.get().len()}else{0}}" "{t("条","segments")}</span></div><div class="record-list"><Failure error=error/>
        <Show when=move||state.get()=="loading"><p role="status">{t("正在查询录像…","Loading recordings…")}</p></Show><Show when=move||!current()||state.get()=="idle"><p class="empty-state">{t("选择时间范围后查询录像。","Choose a time range and search for recordings.")}</p></Show><Show when=move||current()&&state.get()=="ready"&&spans.get().is_empty()><p class="empty-state">{t("所选范围内没有录像","No recordings in the selected range")}</p></Show>
        <For each=move||if current(){spans.get()}else{vec![]} key=|span|(span.start.clone(),span.duration.to_bits()) children=move|span|{let playing=span.clone();view!{<button class="xcss-button record-item" on:click=move |_|{selected.set(Some(playing.clone()));attempt.update(|v|*v+=1);}><span>{date(&span.start)}</span><strong>{duration(span.duration)}</strong><i>{t("播放","Play")}</i></button>}}/>
        </div></div><div class="playback-stage"><For each={move||vec![(playback.get(),attempt.get())]} key=|key|key.clone() children=move|(source,_)|view!{<video src=source controls playsinline autoplay/>}/><div>{move||if current(){selected.get().map(|span|format!("{} · {}",date(&span.start),duration(span.duration))).unwrap_or(t("尚未选择录像","No recording selected"))}else{t("尚未选择录像","No recording selected")}}</div></div></div>
    </section>}
}

#[component]
fn CameraSelect(cameras: Signal<Vec<Camera>>, selected: RwSignal<String>) -> impl IntoView {
    let open = RwSignal::new(false);
    view! {<div class="xcss-select" style:position="relative"><select class="xcss-select-native" tabindex="-1" aria-hidden="true" prop:value=move||selected.get() on:change=move|event|selected.set(event_target_value(&event))><For each=move||cameras.get() key=|c|(c.id.clone(),c.name.clone()) children=move|c|view!{<option value=c.id>{c.name}</option>}/></select>
    <button class="xcss-input xcss-select-trigger" type="button" role="combobox" aria-label=t("摄像头","Camera") aria-haspopup="listbox" aria-controls="xcos-recording-camera-options" aria-expanded=move||open.get().to_string() on:click=move |_|open.update(|v|*v = !*v) on:keydown=move|event:web_sys::KeyboardEvent|{let options=cameras.get_untracked();let index=options.iter().position(|c|c.id==selected.get_untracked()).unwrap_or_default();let next=match event.key().as_str(){"ArrowDown"=>Some((index+1).min(options.len().saturating_sub(1))),"ArrowUp"=>Some(index.saturating_sub(1)),"Home"=>Some(0),"End"=>Some(options.len().saturating_sub(1)),"Escape"=>{open.set(false);event.prevent_default();None},_=>None};if let Some(index)=next{event.prevent_default();if let Some(camera)=options.get(index){selected.set(camera.id.clone())}}}><span class="xcss-select-value">{move||cameras.get().into_iter().find(|c|c.id==selected.get()).map(|c|c.name).unwrap_or_default()}</span><svg class="xcss-select-arrow" aria-hidden="true" viewBox="0 0 16 16"><path d="m6 3 5 5-5 5" stroke="currentColor"/></svg></button>
    <div hidden=move||!open.get() id="xcos-recording-camera-options" class="xcss-select-panel" role="listbox" aria-label=t("摄像头","Camera") style:position="absolute" style:top="100%" style:left="0" style:right="0" style:z-index="10" style:max-height="18rem"><For each=move||cameras.get() key=|c|(c.id.clone(),c.name.clone()) children=move|c|{let id=c.id.clone();let choice=c.id;view!{<button type="button" class="xcss-select-option" role="option" aria-selected=move||(selected.get()==id).to_string() on:click=move |_|{selected.set(choice.clone());open.set(false);}>{c.name}</button>}}/></div></div>}
}
