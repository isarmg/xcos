import { registerHooks } from "node:module";
import { assertCommittedLogRequests } from "./date-range.mjs";
import assert from "node:assert/strict";
import test from "node:test";
import { loadOriginalMain } from "./fixtures/original-main.mjs";
import { HookHost, walk, textContent } from "./fixtures/hook-host.mjs";

const { Console, ClientsView, ClientSettings, RecordingsView, CameraDrawer, utcDateInput, formatDate } = await loadOriginalMain();
const settle = () => new Promise(resolve => setImmediate(resolve));
function deferred() {
  let resolve, reject;
  const promise = new Promise((done, fail) => { resolve = done; reject = fail; });
  return { promise, resolve, reject };
}
const segment = { start: "2026-10-01T12:00:00Z", duration: 60 };
function recordings(t) {
  const host = new HookHost(RecordingsView, {
    cameras: [{ id: "camera-a", name: "A" }, { id: "camera-b", name: "B" }],
    toast() {},
  });
  t.after(() => host.unmount());
  const nodes = predicate => walk(host.render(), predicate);
  const date = (index, value) => nodes(node => node.type === "input")[index].props.onChange({ target: { value } });
  const range = (day = "01") => { date(0, `2026-10-${day}T00:00`); date(1, `2026-10-${day}T23:59`); };
  const search = () => nodes(node => node.type === "button" && textContent(node) === "Search recordings")[0].props.onClick();
  const rows = () => nodes(node => node.props.className === "record-item");
  const video = () => nodes(node => node.type === "video")[0];
  range();
  return { host, nodes, date, range, search, rows, video };
}

test("recordings distinguish initial, loading, failed, retry, and empty results", async t => {
  const { host, nodes, range, search, rows, video } = recordings(t);
  assert.match(textContent(host.render()), /Choose a time range and search/);
  assert.doesNotMatch(textContent(host.render()), /No recordings in the selected range/);
  globalThis.probeRequest = async () => [segment];
  search();
  assert.equal(nodes(node => node.type === "loading").length, 1);
  await settle();
  rows()[0].props.onClick();
  assert.match(video().props.src, /2026-10-01/);
  const playingKey = video().key;
  const refresh = deferred();
  globalThis.probeRequest = () => refresh.promise;
  search();
  assert.equal(rows().length, 0, "a repeated query clears the previous result immediately");
  assert.equal(video().props.src, undefined);
  assert.notEqual(video().key, playingKey, "clearing playback replaces the media element, not just its src attribute");
  const clearedKey = video().key;
  refresh.resolve([segment]);
  await settle();
  rows()[0].props.onClick();
  assert.match(video().props.src, /2026-10-01/, "selecting the same recording after reset restores its media source");
  assert.notEqual(video().key, clearedKey, "selecting after reset mounts its media element again");
  const resumedKey = video().key;
  range("02");
  assert.notEqual(video().key, resumedKey, "editing dates unmounts the active recording");
  const failed = deferred();
  globalThis.probeRequest = () => failed.promise;
  search();
  assert.equal(rows().length, 0);
  assert.equal(video().props.src, undefined);
  failed.reject(new Error("Temporary request failure"));
  await settle();
  assert.equal(nodes(node => node.type === "loading").length, 0);
  assert.match(textContent(nodes(node => node.type === "error")[0]), /could not be loaded/);
  assert.equal(rows().length, 0);
  globalThis.probeRequest = async () => [];
  search();
  await settle();
  assert.equal(nodes(node => node.type === "error").length, 0);
  assert.match(textContent(host.render()), /No recordings in the selected range/);
});

test("playing the same recording again restarts its media element without another search", async t => {
  const { host, search, rows, video } = recordings(t);
  let searches = 0;
  const other = { start: "2026-10-01T13:00:00Z", duration: 120 };
  globalThis.probeRequest = async () => { searches += 1; return [segment, other]; };
  search();
  await settle();
  rows()[0].props.onClick();
  const source = video().props.src;
  let previousKey = video().key;
  for (let attempt = 0; attempt < 3; attempt += 1) {
    rows()[0].props.onClick();
    assert.notEqual(video().key, previousKey, "Play must restart even an already selected or ended segment");
    assert.equal(video().props.src, source);
    assert.equal(video().props.autoPlay, true);
    previousKey = video().key;
  }
  host.props = { ...host.props, cameras: host.props.cameras.map(camera => ({ ...camera, name: `${camera.name} refreshed` })) };
  assert.equal(video().key, previousKey, "background snapshots must not restart playback");
  rows()[1].props.onClick();
  assert.notEqual(video().key, previousKey);
  assert.equal(new URL(video().props.src, "https://camera.example").searchParams.get("start"), other.start);
  assert.equal(searches, 1);
});

test("invalid or reversed recording dates fail locally and allow a retry", async t => {
  const { nodes, date, range, search, rows } = recordings(t);
  let calls = 0;
  globalThis.probeRequest = async () => { calls += 1; return [segment]; };
  for (const start of ["", "not-a-date", "2026-10-03T00:00"]) {
    date(0, start);
    search();
    await settle();
    assert.equal(nodes(node => node.type === "loading").length, 0);
    assert.equal(nodes(node => node.type === "error").length, 1);
    assert.equal(calls, 0);
  }
  range();
  date(1, "");
  search();
  await settle();
  assert.equal(nodes(node => node.type === "error").length, 1);
  assert.equal(calls, 0);
  range();
  search();
  await settle();
  assert.equal(calls, 1);
  assert.equal(rows().length, 1);
});

test("recording date edits and newer searches ignore old success and failure", async t => {
  const { host, nodes, range, search, rows } = recordings(t);
  const old = deferred(), latest = deferred();
  globalThis.probeRequest = () => old.promise;
  search();
  range("02");
  host.render();
  old.resolve([segment]);
  await settle();
  assert.equal(rows().length, 0);
  assert.match(textContent(host.render()), /Choose a time range and search/);
  const superseded = deferred();
  globalThis.probeRequest = () => superseded.promise;
  search();
  globalThis.probeRequest = () => latest.promise;
  search();
  latest.resolve([{ ...segment, start: new Date("2026-10-02T12:00:00Z").toISOString() }]);
  await settle();
  superseded.reject(new Error("Old request failure"));
  await settle();
  assert.equal(nodes(node => node.type === "error").length, 0);
  assert.equal(rows().length, 1);
  assert.match(textContent(rows()[0]), /10\/02\/2026/);
});

test("switching recording camera clears playback and ignores its pending request", async t => {
  const { host, nodes, search, rows, video } = recordings(t);
  globalThis.probeRequest = async () => [segment];
  search();
  await settle();
  rows()[0].props.onClick();
  assert.match(video().props.src, /camera-a/);
  const old = deferred();
  globalThis.probeRequest = () => old.promise;
  search();
  nodes(node => node.type === "select")[0].props.onChange({ target: { value: "camera-b" } });
  host.render();
  old.resolve([segment]);
  await settle();
  assert.equal(rows().length, 0);
  assert.equal(video().props.src, undefined);
  assert.match(textContent(host.render()), /Choose a time range and search/);
});

for (const change of ["camera", "camera replacement", "start", "end"]) {
  test(`recording ${change} changes never render the old result before effects clear it`, async t => {
    const { host, nodes, date, search, rows, video } = recordings(t);
    globalThis.probeRequest = async () => [segment];
    search();
    await settle();
    rows()[0].props.onClick();
    assert.match(video().props.src, /camera-a/);

    if (change === "camera") nodes(node => node.type === "select")[0].props.onChange({ target: { value: "camera-b" } });
    else if (change === "camera replacement") host.props = { ...host.props, cameras: [{ id: "camera-b", name: "B" }] };
    else date(change === "start" ? 0 : 1, "2026-10-02T00:00");

    let renders = 0;
    host.render(tree => {
      renders += 1;
      assert.equal(walk(tree, node => node.type === "video")[0].props.src, undefined,
        "a committed render must not assign a stale segment to another camera or date range");
      assert.equal(walk(tree, node => node.props.className === "record-item").length, 0);
      assert.match(textContent(tree), /Choose a time range and search/);
      assert.match(textContent(tree), /No recording selected/);
    });
    assert.ok(renders > 0);
  });
}

test("refreshing the same camera snapshot preserves its selected recording", async t => {
  const { host, search, rows, video } = recordings(t);
  globalThis.probeRequest = async () => [segment];
  search();
  await settle();
  rows()[0].props.onClick();
  const selectedSource = video().props.src;
  host.props = { ...host.props, cameras: host.props.cameras.map(camera => ({ ...camera, name: `${camera.name} refreshed` })) };
  host.render(tree => {
    assert.equal(walk(tree, node => node.type === "video")[0].props.src, selectedSource);
    assert.equal(walk(tree, node => node.props.className === "record-item").length, 1);
  });
});

test("a slow earlier refresh cannot replace a newer result or its playback selection", async t => {
  const { host, search, rows, video } = recordings(t);
  const earlier = deferred(), newer = deferred();
  globalThis.probeRequest = () => earlier.promise;
  search();
  globalThis.probeRequest = () => newer.promise;
  search();
  const newerSpan = { start: "2026-10-01T13:00:00Z", duration: 120 };
  newer.resolve([newerSpan]);
  await settle();
  rows()[0].props.onClick();
  const selectedSource = video().props.src;
  earlier.resolve([segment]);
  await settle();
  host.render(tree => {
    const source = walk(tree, node => node.type === "video")[0].props.src;
    assert.equal(source, selectedSource);
    assert.equal(new URL(source, "https://camera.example").searchParams.get("start"), newerSpan.start);
    assert.equal(walk(tree, node => node.props.className === "record-item").length, 1);
    assert.match(textContent(tree), /2m 0s/);
  });
});

test("cross-midnight recording searches preserve the requested bounds and selected segment", async t => {
  const { date, search, rows, video } = recordings(t);
  date(0, "2026-10-01T23:30");
  date(1, "2026-10-02T00:30");
  const span = { start: new Date("2026-10-01T23:45Z").toISOString(), duration: 1800 };
  let queried;
  globalThis.probeRequest = async path => { queried = new URL(path, "https://camera.example"); return [span]; };
  search();
  await settle();
  assert.equal(queried.searchParams.get("start"), new Date("2026-10-01T23:30Z").toISOString());
  assert.equal(queried.searchParams.get("end"), new Date("2026-10-02T00:30Z").toISOString());
  rows()[0].props.onClick();
  const playback = new URL(video().props.src, "https://camera.example");
  assert.equal(playback.searchParams.get("start"), span.start);
  assert.equal(playback.searchParams.get("duration"), "1800");
});

test("recording UTC date inputs preserve absolute instants", () => {
  for (const iso of ["2024-02-29T23:59:00.000Z", "2026-01-01T00:01:00.000Z"]) {
    assert.equal(new Date(`${utcDateInput(new Date(iso))}Z`).toISOString(), iso);
  }
  assert.equal(formatDate("2026-10-08T16:00:00+08:00"), formatDate("2026-10-08T08:00:00Z"));
  assert.match(formatDate("2026-10-08T08:00:00Z"), / UTC$/);
});

test("recording date fields submit UTC instants without changing the selected hour", async t => {
  const { date, search } = recordings(t);
  let requested;
  globalThis.probeRequest = async path => { requested = new URL(path, "https://example.test"); return []; };
  date(0, "2024-02-29T23:58");
  date(1, "2024-03-01T00:01");
  search();
  await settle();
  assert.equal(requested.searchParams.get("start"), "2024-02-29T23:58:00.000Z");
  assert.equal(requested.searchParams.get("end"), "2024-03-01T00:01:00.000Z");
});

test("retrying a failed PTZ status read only repeats GET, never its physical command", async t => {
  const timers = new Map(), requests = [];
  let nextTimer = 0, fail = true, successfulReads = 0;
  t.mock.method(window, "setTimeout", fn => { timers.set(++nextTimer, fn); return nextTimer; });
  t.mock.method(window, "clearTimeout", id => timers.delete(id));
  const camera = { id: "camera-a", name: "A", location: "", adapter_kind: "onvif", capabilities: { ptz: "supported" } };
  const host = new HookHost(CameraDrawer, { camera, close() {}, toast() {} });
  t.after(() => host.unmount());
  const find = predicate => walk(host.render(), predicate)[0];
  const poll = async () => {
    assert.equal(timers.size, 1);
    const [id, fn] = [...timers.entries()][0];
    timers.delete(id); fn(); await settle(); host.render();
  };
  globalThis.probeRequest = async (path, _guard, options) => {
    requests.push({ path, method: options?.method ?? "GET" });
    if (options?.method === "POST") return { command_id: "stop-1", state: "queued" };
    if (fail) throw new Error("Transient read failure");
    successfulReads += 1;
    return { command_id: "stop-1", state: successfulReads === 1 ? "awaiting_result" : "succeeded" };
  };
  find(node => node.props["aria-label"] === "Stop movement").props.onClick();
  await settle(); host.render();
  await poll();
  assert.match(textContent(find(node => node.type === "error")), /could not be checked/);
  assert.equal(find(node => node.props.role === "status"), undefined);
  assert.equal(timers.size, 0);
  fail = false;
  find(node => node.type === "button" && textContent(node) === "Retry status lookup").props.onClick();
  host.render(); await poll();
  assert.equal(textContent(find(node => node.props.role === "status")), "Sent; waiting for the device result");
  await poll();
  assert.equal(find(node => node.type === "error"), undefined);
  assert.equal(textContent(find(node => node.props.role === "status")), "The device confirmed execution");
  assert.equal(timers.size, 0);
  assert.deepEqual(requests.map(request => request.method), ["POST", "GET", "GET", "GET"]);
  assert.equal(requests[1].path, requests[2].path);
});


test("PTZ controls follow supported, unsupported, and unknown capabilities", () => {
  for (const support of ["supported", "unsupported", "unknown"]) {
    const camera = { id: "camera-a", name: "A", location: "", adapter_kind: "onvif", capabilities: { ptz: support } };
    const host = new HookHost(CameraDrawer, { camera, close() {}, toast() {} });
    const panel = walk(host.render(), node => node.props["aria-label"] === "PTZ controls");
    assert.equal(panel.length, support === "supported" ? 1 : 0);
    host.unmount();
  }
});


function logConsole(t, responder, hash = "#logs") {
  const sources = [], calls = [];
  const oldHash = window.location.hash;
  window.location.hash = hash;
  t.after(() => { window.location.hash = oldHash; });
  const previousSource = globalThis.EventSource;
  globalThis.EventSource = class {
    listeners = new Map();
    constructor() { sources.push(this); }
    addEventListener(name, callback) { this.listeners.set(name, callback); }
    close() {}
    emit(name) { this.listeners.get(name)?.({}); }
  };
  t.after(() => { globalThis.EventSource = previousSource; });
  globalThis.probeRequest = async (path, guard, signal) => {
    calls.push({ path, signal });
    if (path === "/logs/calendar") return { today: "2026-10-11" };
    return responder(path);
  };
  const host = new HookHost(Console);
  t.after(() => host.unmount());
  const render = () => host.render();
  const ready = async () => { render(); await settle(); render(); await settle(); render(); };
  const emit = name => sources.at(-1).emit(name);
  const open = () => emit("open");
  const nodes = predicate => walk(render(), predicate);
  return { calls, render, ready, open, emit, nodes };
}
const historyPage = (id, next_cursor = null) => ({ format: "xcos-history-page-v1", items: [{ id }], next_cursor });

test("ordinary event-stream reconnect reloads visible history missed during disconnect", async t => {
  let revision = 1;
  const logs = logConsole(t, () => historyPage(`revision-${revision}`));
  await logs.ready();
  const events = () => logs.nodes(node => typeof node.type === "function" && node.type.name === "EventsView")[0].props.events;
  assert.equal(events()[0].id, "revision-1");
  const before = logs.calls.length;
  revision = 2;
  // Ordinary transport reconnect emits open, not the server's lag-only
  // resync-required notification. This server does not replay missed events.
  logs.open(); await settle(); logs.render();
  assert.equal(logs.calls.length, before + 3);
  assert.equal(events()[0].id, "revision-2");
});

test("event-stream reconnect preserves the selected log range and page cursor", async t => {
  const logs = logConsole(t, () => historyPage("row", "next-event-page"));
  await logs.ready();
  const dateField = () => logs.nodes(node => node.type === "date-range")[0];
  dateField().props.onApply({ start: "2026-10-01", end: "2026-10-03" });
  dateField().props.onValidityChange(true);
  await logs.ready();
  const navigation = logs.nodes(node => node.props["aria-label"] === "Event history pages")[0];
  walk(navigation, node => node.type === "button" && textContent(node) === "Next page")[0].props.onClick();
  await logs.ready();
  const selectedRequests = logs.calls.slice(-3).map(call => call.path);
  assert.match(selectedRequests[0], /start_date=2026-10-01&end_date=2026-10-03&cursor=next-event-page/);
  logs.open(); await settle(); logs.render();
  assert.deepEqual(logs.calls.slice(-3).map(call => call.path), selectedRequests);
});

test("reconnect does not query logs off-screen and supersedes stale in-flight history", async t => {
  const offscreen = logConsole(t, () => historyPage("unused"), "#details");
  await offscreen.ready();
  offscreen.open(); await settle(); offscreen.render();
  assert.equal(offscreen.calls.length, 0);

  let response = () => historyPage("original");
  const logs = logConsole(t, () => response());
  await logs.ready();
  const old = deferred();
  response = () => old.promise;
  logs.open(); logs.render();
  const staleRequests = logs.calls.slice(-3);
  response = () => historyPage("newest");
  logs.open(); await settle(); logs.render();
  assert.ok(staleRequests.every(call => call.signal.aborted));
  old.resolve(historyPage("stale")); await settle(); logs.render();
  const events = logs.nodes(node => typeof node.type === "function" && node.type.name === "EventsView")[0].props.events;
  assert.equal(events[0].id, "newest");
});


for (const event of ["open", "resync-required", "system-event"]) {
  for (const direction of ["First page", "Previous page", "Next page"]) {
    test(`${event} preserves pending ${direction} navigation instead of restoring the completed page`, async t => {
      let pending = null;
      const page = path => {
        const offset = Number(new URL(path, "https://camera.example").searchParams.get("cursor") ?? 0);
        return { format: "xcos-history-page-v1", items: Array.from({ length: Math.min(100, 220 - offset) }, (_, i) => ({ id: `row-${offset + i}` })), next_cursor: offset < 200 ? String(offset + 100) : null };
      };
      const logs = logConsole(t, path => pending ? pending.promise : page(path));
      await logs.ready();
      logs.nodes(node => node.type === "date-range")[0].props.onValidityChange(true);
      const click = name => {
        const navigation = logs.nodes(node => node.props["aria-label"] === "Event history pages")[0];
        const button = walk(navigation, node => node.type === "button" && textContent(node) === name)[0];
        assert.equal(button.props.disabled, false);
        button.props.onClick();
      };
      // Start on the 20-row final page for First/Previous, or page 2 for Next.
      click("Next page"); await logs.ready();
      if (direction !== "Next page") { click("Next page"); await logs.ready(); }
      const delayed = deferred();
      pending = delayed;
      click(direction);
      logs.render();
      const navigationRequests = logs.calls.slice(-3);
      pending = null;
      logs.emit(event); await logs.ready();
      assert.deepEqual(logs.calls.slice(-3).map(call => call.path), navigationRequests.map(call => call.path), "stream refresh must retain the user's in-flight destination");
      assert.ok(navigationRequests.every(call => call.signal.aborted));
      const events = () => logs.nodes(node => typeof node.type === "function" && node.type.name === "EventsView")[0].props.events;
      const offset = direction === "First page" ? 0 : direction === "Previous page" ? 100 : 200;
      assert.equal(events().length, offset === 200 ? 20 : 100);
      assert.equal(events()[0].id, `row-${offset}`);
      // Even a transport that completes despite abort must not restore stale rows.
      delayed.resolve(historyPage("stale")); await logs.ready();
      assert.equal(events()[0].id, `row-${offset}`);
      if (offset !== 0) {
        click("Previous page"); await logs.ready();
        assert.equal(events()[0].id, `row-${offset - 100}`, "cursor history must follow the pending navigation too");
      } else {
        const navigation = logs.nodes(node => node.props["aria-label"] === "Event history pages")[0];
        assert.equal(walk(navigation, node => node.type === "button" && textContent(node) === "Previous page")[0].props.disabled, true);
      }
    });
  }
}


test("date-query guard allows only unchanged committed queries during draft editing", () => {
  const before = ["/events/logs", "/media/operations", "/audit"].map(path => `${path}?date=2026-09-03`);
  assertCommittedLogRequests([...before, ...before], before);
  for (const query of ["date=2022-02-30", "date=2024-13-01", "date=0000-01-01", "start_date=2023-02-02&end_date=2022-02-01", "start_date=2026-09-03", "start_date=2026-09-03&end_date=2026-09-04", "date=2026-09-03&end_date=2026-09-04"]) {
    for (const endpoint of ["/events/logs", "/media/operations", "/audit"]) {
      assert.throws(() => assertCommittedLogRequests([...before, ...before, `${endpoint}?${query}`], before), /draft edits must never change/);
    }
  }
});

test("real date editor keeps invalid and unapplied drafts local while SSE refreshes committed logs", async t => {
  const hooks = registerHooks({ resolve(specifier, context, next) {
    if (specifier === "react" || specifier === "react/jsx-runtime") return { url: new URL("./fixtures/hook-host.mjs", import.meta.url).href, shortCircuit: true };
    if (specifier === "./i18n.js") return { url: "data:text/javascript,export const t=(zh,en)=>en", shortCircuit: true };
    return next(specifier, context);
  } });
  let DateRangeField;
  try { ({ DateRangeField } = await import("@xcss/web/admin-ui/date-range")); } finally { hooks.deregister(); }
  const logs = logConsole(t, () => historyPage("row"));
  await logs.ready();
  const date = new HookHost(DateRangeField, logs.nodes(node => node.type === "date-range")[0].props);
  t.after(() => date.unmount());
  const draft = (start, end) => {
    for (const [endpoint, value] of [start, end].entries()) {
      for (const [field, part] of value.split("-").entries()) {
        walk(date.render(), node => node.type === "input")[endpoint * 3 + field].props.onChange({ target: { value: part } });
      }
    }
    date.render(); logs.render();
  };
  const apply = () => walk(date.render(), node => node.props.onKeyDown)[0].props.onKeyDown({ key: "Enter", nativeEvent: {}, preventDefault() {} });
  const requests = () => logs.calls.map(call => call.path).filter(path => path.includes("?"));
  const before = requests();
  for (const [start, end] of [["2022-02-30", "2023-02-02"], ["2024-13-01", "2025-01-01"], ["0000-01-01", "2025-01-01"], ["2023-02-02", "2022-02-01"]]) {
    draft(start, end);
    const count = requests().length;
    apply(); await logs.ready();
    assert.equal(requests().length, count, "invalid Enter causes no request");
    logs.open(); await logs.ready();
    assert.equal(requests().length, count + 3, "SSE still refreshes all committed logs");
    assertCommittedLogRequests(requests(), before);
  }
  const beforeDraft = requests().length;
  draft("2026-10-01", "2026-10-03");
  await logs.ready();
  assert.equal(requests().length, beforeDraft, "a valid draft also waits for Enter");
  assertCommittedLogRequests(requests(), before);
  logs.open(); await logs.ready();
  assertCommittedLogRequests(requests(), before);
  apply(); await logs.ready();
  assert.ok(requests().slice(-3).every(path => path.endsWith("?start_date=2026-10-01&end_date=2026-10-03")));
});


test("instance cancellation retries use only revoke and permanent deletion has a separate confirmation", async t => {
  const client = { id: "instance-a", name: "Camera A", status: "pending" };
  const calls = [];
  let serverStatus = "pending", changed = 0, lost = true;
  globalThis.probeRequest = async (path, _guard, options) => {
    calls.push({ path, method: options.method });
    if (path.endsWith("/delete")) { assert.equal(serverStatus, "revoked"); serverStatus = "deleted"; }
    else { serverStatus = "revoked"; if (lost) { lost = false; throw new Error("Response lost after commit"); } }
  };
  const host = new HookHost(ClientsView, { clients: [client], cameras: [], changed: async () => { changed += 1; }, toast() {}, select() {} });
  t.after(() => host.unmount());
  const button = label => walk(host.render(), node => node.type === "button" && textContent(node) === label)[0];
  const confirm = () => walk(host.render(), node => node.type === "confirm")[0];
  assert.equal(button("Permanently delete").props.disabled, true);
  button("Cancel pairing").props.onClick(); confirm().props.onConfirm(); await settle();
  assert.equal(serverStatus, "revoked");
  // List is still stale after losing the response: repeating the same operation is safe.
  button("Cancel pairing").props.onClick(); confirm().props.onConfirm(); await settle();
  assert.deepEqual(calls.map(call => call.path), ["/clients/instance-a", "/clients/instance-a"]);
  assert.equal(serverStatus, "revoked");
  assert.equal(changed, 1);
  host.props = { ...host.props, clients: [{ ...client, status: "revoked" }] };
  assert.equal(button("Permanently delete").props.disabled, false);
  button("Permanently delete").props.onClick();
  assert.equal(calls.length, 2, "opening confirmation must not delete");
  button("Confirm delete").props.onClick(); await settle();
  assert.equal(serverStatus, "deleted");
  assert.deepEqual(calls[2], { path: "/clients/instance-a/delete", method: "DELETE" });
});

test("revoked instance rename is explained and cannot send a PATCH even through form submission", async t => {
  let calls = 0;
  globalThis.probeRequest = async () => { calls += 1; };
  const host = new HookHost(ClientSettings, { client: { id: "revoked-a", name: "Original", status: "revoked" }, changed: async () => {}, toast() {} });
  t.after(() => host.unmount());
  assert.match(textContent(host.render()), /revoked instance cannot be renamed/i);
  const input = walk(host.render(), node => node.type === "input")[0];
  assert.equal(input.props.readOnly, true);
  input.props.onChange({ target: { value: "Changed" } });
  const save = walk(host.render(), node => node.type === "button")[0];
  assert.equal(save.props.disabled, true);
  walk(host.render(), node => node.type === "form")[0].props.onSubmit({ preventDefault() {} });
  await settle();
  assert.equal(calls, 0);
});
