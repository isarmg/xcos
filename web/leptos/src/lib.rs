mod controls;
mod logs;
mod media;
mod model;
mod recordings;

use leptos::prelude::*;
use model::*;
use serde_json::json;
use wasm_bindgen::{JsCast, prelude::*};
use wasm_bindgen_futures::spawn_local;
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
    xcss_web::mount(
        "root",
        || view! {<AdminShell product="xcos" landing="#instances"><Console/></AdminShell>},
    );
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
    view! {<div class="xcos-business xcss-content-stack">
        <HeaderActions><button class="xcss-button" disabled=move||app.busy.get() on:click=move |_|{navigate("instances");app.mutate("/clients".into(),"POST",Some(json!({"name":t("新实例","New instance")})),t("客户端实例已创建","Client instance created"))}>{t("新建实例","Create instance")}</button><button class="xcss-button" disabled=move||app.busy.get() on:click=move |_|{app.refresh();app.logs_refresh.update(|v|*v+=1)}>{t("刷新","Refresh")}</button></HeaderActions>
        <HeaderNavigation>{[ ("instances","实例列表","Instance list"),("details","详细信息","Details"),("logs","日志","Logs")].into_iter().map(|(id,zh,en)|view!{<button class="xcss-button" aria-pressed=move||(page.get()==id).to_string() disabled=move||id=="details"&&chosen.get().is_none() on:click=move |_|navigate(id)>{t(zh,en)}</button>}).collect_view()}</HeaderNavigation>
        <Failure error=app.error/>
        {move||match page.get().as_str(){"account"=>view!{<AccountPage/>}.into_any(),"details"=>view!{<For each=move||chosen.get().map(|c|vec![c.id]).unwrap_or_default() key=|id|id.clone() children=move |_|view!{<Details client=chosen.get_untracked().unwrap() inspect=inspect/>}/>} .into_any(),"logs"=>view!{<logs::Logs/>}.into_any(),_=>view!{<Statistics/><Clients/>}.into_any()}}
        {move||drawer_id.get().map(|id|view!{<controls::CameraDrawer id=id close=Callback::new(move|_|inspect.set(None))/>})}
    </div>}
}

#[component]
pub fn Failure(error: RwSignal<Option<xcss_web::api::ApiError>>) -> impl IntoView {
    view! {<Show when=move||error.get().is_some()><div class="xcss-error-state" role="alert"><p>{move||error.get().map(|e|e.to_string()).unwrap_or_default()}</p>{move||error.get().and_then(|e|e.request_id).map(|id|view!{<small>{t("请求标识：","Request ID: ")}{id}</small>})}</div></Show>}
}
#[component]
fn Statistics() -> impl IntoView {
    let app = expect_context::<AppState>();
    view! {<section class="view active xcss-content-stack"><div class="xcss-table-scroll"><table class="xcss-table xcss-statistics-table" aria-label=t("实例统计","Instance statistics")><thead><tr><th>{t("统计项","Metric")}</th><th>{t("总数 / 在线","Total / online")}</th></tr></thead><tbody><tr><th scope="row">{t("总数","Total")}</th><td>{move||format!("{} / {}",app.clients.get().len(),app.clients.get().iter().filter(|c|c.status=="online").count())}</td></tr><tr><th scope="row">{t("已上报摄像机","Reported cameras")}</th><td>{move||app.cameras.get().len()}</td></tr><tr><th scope="row">{t("媒体服务","Media service")}</th><td>{move||app.system.get().map(|s|if s.media_service=="ok"{t("正常","Available")}else{t("不可用","Unavailable")}).unwrap_or(t("读取中","Loading"))}</td></tr><tr><th scope="row">{t("已配置服务器录像","Server recordings configured")}</th><td>{move||app.system.get().map(|s|s.cameras.recording_configured.to_string()).unwrap_or("—".into())}</td></tr></tbody></table></div></section>}
}
#[component]
fn Clients() -> impl IntoView {
    let app = expect_context::<AppState>();
    let candidate = RwSignal::new(String::new());
    let confirm = RwSignal::new(None::<(Client, String)>);
    view! {<div class="xcss-content-stack"><div class="xcss-table-scroll"><table class="xcss-table" aria-label=t("摄像机实例列表","Camera instance list")><thead><tr>{[("实例名称","Instance name"),("配对状态","Pairing status"),("摄像机状态","Camera status"),("厂商 / 型号","Make / model"),("操作","Actions"),("删除","Delete")].into_iter().map(|(zh,en)|view!{<th>{t(zh,en)}</th>}).collect_view()}</tr></thead><tbody>
        <Show when=move||app.clients.get().is_empty()><tr><td colspan="6" class="empty-state">{t("尚无摄像机实例","No camera instances")}</td></tr></Show>
        <For each={move||app.clients.get().into_iter().map(|c|c.id).collect::<Vec<_>>()} key=|id|id.clone() children=move|id|{
            let cid=id.clone();let client=Memo::new(move |_|app.clients.get().into_iter().find(|c|c.id==cid));let camera=Memo::new(move |_|app.cameras.get().into_iter().find(|c|c.client_id.as_deref()==Some(id.as_str())));
            view!{<tr><th scope="row"><a class="xcss-instance-link" href="#details" aria-label=move||format!("{} {}",t("选择实例","Select instance"),client.get().map(|c|c.name).unwrap_or_default()) on:click=move |_|{if let Some(c)=client.get_untracked(){app.selected.set(c.id)}}>{move||client.get().map(|c|c.name).unwrap_or_default()}</a></th><td>{move||client.get().map(|c|label(&c.status))}</td><td>{move||camera.get().map(|_|view!{<CameraStatus camera=Signal::derive(move||camera.get())/>}).map(|view|view.into_any()).unwrap_or_else(||t("尚未上报","Not yet reported").into_any())}</td><td>{move||camera.get().map(|c|make(&c)).unwrap_or("—".into())}</td><td><div class="xcss-actions"><Show when=move||client.get().is_some_and(|c|c.status!="revoked")><button class="xcss-button" disabled=move||app.busy.get() on:click=move |_|{if let Some(c)=client.get_untracked(){confirm.set(Some((c,"rotate".into())))}}>{t("更换密码","Change password")}</button><button class="xcss-button" disabled=move||app.busy.get() on:click=move |_|{if let Some(c)=client.get_untracked(){confirm.set(Some((c,"revoke".into())))}}>{move||if client.get().is_some_and(|c|c.status=="pending"){t("取消配对","Cancel pairing")}else{t("撤销实例","Revoke instance")}}</button></Show></div></td><td><div class="xcss-actions"><Show when=move||client.get().is_some_and(|c|candidate.get()==c.id) fallback=move||view!{<button class="xcss-button" disabled=move||app.busy.get()||client.get().is_none_or(|c|c.status!="revoked") on:click=move |_|{if let Some(c)=client.get_untracked(){candidate.set(c.id)}}>{t("永久删除","Permanently delete")}</button>}><button class="xcss-button" on:click=move |_|candidate.set(String::new())>{t("取消","Cancel")}</button><button class="xcss-button xcss-danger" disabled=move||app.busy.get() on:click=move |_|{if let Some(c)=client.get_untracked()&& c.status=="revoked"{app.mutate(format!("/clients/{}/delete",c.id),"DELETE",None,t("实例已永久删除","Instance permanently deleted"));candidate.set(String::new());}}>{t("确认删除","Confirm delete")}</button></Show></div></td></tr>}
        }/>
    </tbody></table></div>
    {move||confirm.get().map(|(client,action)|{let rotate=action=="rotate";view!{<Confirm title=if rotate{t("更换密码","Change password")}else{t("撤销实例","Revoke instance")} description=if rotate{t("这会立即撤销当前客户端凭据并停用它管理的摄像头。客户端必须使用新密码重新配对并重新上报摄像头。","This immediately revokes the current client credential and disables its cameras. The client must pair again with the new password and report its cameras again.")}else{t("当前密码和客户端凭据将失效；重复撤销不会删除实例信息。媒体路径清理完成后，需单独确认永久删除。","The password and client credential will be invalidated. Repeating revocation preserves the instance entry. After media-path cleanup, permanent deletion requires a separate confirmation.")} close=Callback::new(move|_|confirm.set(None)) accept=Callback::new(move|_|{confirm.set(None);if rotate{app.mutate(format!("/clients/{}/authorization",client.id),"PUT",Some(json!({"authorization_code":random_code()})),t("实例密码已更换；原客户端必须使用新密码重新配对","Instance password changed; the old client must pair again with the new password"));}else{app.mutate(format!("/clients/{}",client.id),"DELETE",None,t("实例已撤销；媒体路径清理完成后，可单独确认永久删除","Instance revoked; after media-path cleanup, confirm permanent deletion separately"));}})/>}})}
    </div>}
}

#[component]
pub fn Confirm(
    title: String,
    description: String,
    close: Callback<()>,
    accept: Callback<()>,
) -> impl IntoView {
    let dialog = NodeRef::<leptos::html::Dialog>::new();
    Effect::new(move |_| {
        if let Some(dialog) = dialog.get() {
            let _ = dialog.show_modal();
        }
    });
    view! {<dialog node_ref=dialog on:cancel=move|event:web_sys::Event|{event.prevent_default();close.run(());} class="xcss-dialog" aria-modal="true" aria-label=title.clone()><h2>{title.clone()}</h2><p>{description}</p><div class="xcss-actions"><button class="xcss-button" on:click=move |_|close.run(())>{t("取消","Cancel")}</button><button class="xcss-button xcss-danger" on:click=move |_|accept.run(())>{t("确认","Confirm")}</button></div></dialog>}
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
    view! {<section class="view active xcss-content-stack"><section class="xcss-content-panel" aria-label=t("配对账户信息","Pairing account information")><h2>{move||current.get().map(|c|c.name)}</h2><dl class="xcos-detail-list"><dt>{t("账户名","Account name")}</dt><dd>{move||current.get().map(|c|c.name)}</dd><dt>{t("账户","Account")}</dt><dd><code>{client.id.clone()}</code></dd><dt>{t("密码","Password")}</dt><dd><code>{move||current.get().map(|c|c.authorization_code)}</code></dd><dt>{t("配对状态","Pairing status")}</dt><dd>{move||current.get().map(|c|if c.status=="pending"{t("待配对","Awaiting pairing")}else if c.status=="revoked"{t("已撤销","Revoked")}else{t("已配对","Paired")})}</dd></dl></section>
    <section class="xcss-content-panel" aria-label=t("摄像机状态","Camera status")><h2>{t("摄像机状态","Camera status")}</h2><dl class="xcos-detail-list"><dt>{t("客户端连接","Client connection")}</dt><dd>{move||current.get().map(|c|status_label(&c.status))}</dd><dt>{t("版本","Version")}</dt><dd>{move||current.get().and_then(|c|c.client_version).unwrap_or("—".into())}</dd><dt>{t("最后在线","Last online")}</dt><dd>{move||current.get().and_then(|c|c.last_seen_at).map(|v|date(&v)).unwrap_or("—".into())}</dd><dt>{t("摄像头","Cameras")}</dt><dd>{move||cameras.get().len()}</dd></dl><For each={move||cameras.get().into_iter().map(|c|c.id).collect::<Vec<_>>()} key=|id|id.clone() children=move|id|{let camera=Signal::derive(move||app.cameras.get().into_iter().find(|c|c.id==id));view!{<section><h3>{move||camera.get().map(|c|c.name)}</h3><CameraStatus camera=camera/><Observation camera=camera/></section>}}/></section>
    <div class="command-bar"><div class="search-wrap"><span>"⌕"</span><input class="xcss-input" type="search" aria-label=t("搜索摄像头","Search cameras") placeholder=t("搜索名称或位置","Search by name or location") prop:value=move||search.get() on:input=move|e|search.set(event_target_value(&e))/></div></div>
    <div class="camera-grid"><Show when=move||visible.get().is_empty()><div class="empty-state full-span">{t("该客户端还没有上报匹配的摄像头。","This client has not reported any matching cameras yet.")}</div></Show><For each={move||visible.get().into_iter().map(|c|(c.id,c.enabled,c.has_sub_stream)).collect::<Vec<_>>()} key=|(id,enabled,sub)|(id.clone(),*enabled,*sub) children=move|(id,_,_)|{let cid=id.clone();let camera=Signal::derive(move||app.cameras.get().into_iter().find(|c|c.id==cid));let initial=camera.get_untracked().unwrap();view!{<article class="camera-card xcss-content-panel reveal"><media::LiveVideo camera=initial.clone() profile=if initial.has_sub_stream{"sub".into()}else{"main".into()}/><div class="camera-meta"><div><strong>{move||camera.get().map(|c|c.name)}</strong><small>{move||camera.get().map(|c|format!("{} · {} · {}",if c.location.is_empty(){t("未标注位置","Location not specified")}else{c.location.clone()},make(&c),if c.storage_mode=="server"{t("服务器录像","Server recording")}else{t("客户端录像","Client recording")}))}</small></div><CameraStatus camera=camera/></div><div class="camera-actions"><button class="xcss-button" on:click=move |_|inspect.set(Some(id.clone()))>{t("查看与控制","View and control")}</button></div></article>}}/></div>
    <Show when=move||cameras.get().iter().any(|c|c.storage_mode=="server")><recordings::Recordings cameras=Signal::derive(move||cameras.get().into_iter().filter(|c|c.storage_mode=="server").collect())/></Show>
    <ClientSettings client=Signal::derive(move||current.get())/>
    </section>}
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
    view! {<section class="xcss-content-panel" aria-label=t("实例设置","Instance settings")><h2>{t("实例设置","Instance settings")}</h2><form on:submit=move|e|{e.prevent_default();if pending.get_untracked(){return}let Some(c)=client.get_untracked()else{return};let normalized=name.get_untracked().trim_matches(char::is_whitespace).to_string();if !valid_name(&normalized)||c.status=="revoked"||normalized==synced.get_untracked(){return}pending.set(true);failure.set(None);saved.set(false);spawn_local(async move{let result=request::<Client>(&format!("/clients/{}",c.id),"PATCH",Some(json!({"name":normalized}))).await;if pending.is_disposed(){return}pending.set(false);match result{Ok(updated)=>{synced.set(updated.name);saved.set(true);refresh_saved();},Err(e)=>failure.set(Some(e))}});} aria-busy=move||pending.get().to_string()><label class="xcss-form-field">{t("实例名称","Instance name")}<input class="xcss-input" name="name" required readonly=move||pending.get()||client.get().is_some_and(|c|c.status=="revoked") prop:value=move||name.get() on:input=move|e|name.set(event_target_value(&e))/></label><Show when=move||client.get().is_some_and(|c|c.status=="revoked")><p>{t("已撤销实例不能更改名称；媒体路径清理完成后可永久删除。","A revoked instance cannot be renamed; it can be permanently deleted after media-path cleanup.")}</p></Show><Show when=move||!valid_name(&name.get())><p role="alert">{t("名称须为 1–64 个字符，不能包含控制字符。","Use 1–64 characters without control characters.")}</p></Show><Failure error=failure/><Show when=move||saved.get()&&failure.get().is_some()><p role="alert">{t("实例名称已保存，但列表刷新失败。请重试刷新。","The instance name was saved, but the list could not be refreshed. Please retry the refresh.")}<button class="xcss-button" type="button" on:click=move |_|refresh_saved()>{t("重试刷新","Retry refresh")}</button></p></Show><div class="xcss-actions"><button class="xcss-button" type="submit" disabled=move||pending.get()||client.get().is_none_or(|c|c.status=="revoked")||!valid_name(&name.get())||name.get().trim_matches(char::is_whitespace)==synced.get().trim_matches(char::is_whitespace)>{move||if pending.get(){t("正在保存…","Saving…")}else{t("保存名称","Save name")}}</button></div></form></section>}
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
    view! {<span class="camera-status">{move||{tick.track();camera.get().map(|c|status_label(status(&c,mono()))).unwrap_or_default()}}</span>}
}
#[component]
pub fn Observation(camera: Signal<Option<Camera>>) -> impl IntoView {
    view! {<dl class="xcos-detail-list"><dt>{t("最后有效媒体观测","Last valid media observation")}</dt><dd>{move||camera.get().and_then(|c|c.last_observed_at).map(|s|date(&s)).unwrap_or(t("尚无","None yet"))}</dd><dt>{t("最近保存的媒体状态","Last stored media state")}</dt><dd>{move||camera.get().map(|c|status_label(&c.status))}</dd><dt>{t("客户端最近上报的设备状态","Last client-reported device state")}</dt><dd>{move||camera.get().map(|c|status_label(&c.device_status))}</dd></dl>}
}
