import assert from "node:assert/strict";
import test from "node:test";
import { cameraSnapshot, effectiveStatus, isCameraObservation, remainingObservationMs } from "../src/camera-status.ts";
import { loadOriginalMain } from "./fixtures/original-main.mjs";
import { HookHost, textContent } from "./fixtures/hook-host.mjs";

const { CameraStatus } = await loadOriginalMain();

const observed = "2026-10-10T11:00:00Z";
const expires = "2026-10-10T11:01:00Z";
const receivedAt = 1_000;
const onlineResponse = { enabled: true, status: "online", device_status: "online", observation_status: "fresh", last_observed_at: observed, observation_expires_at: expires, observation_remaining_ms: 30_000 };
const online = cameraSnapshot(onlineResponse, receivedAt);

test("stale or missing observations cannot produce a current online/offline badge", () => {
  assert.equal(effectiveStatus(online, receivedAt), "online");
  for (const status of ["online", "offline", "error"]) {
    assert.equal(effectiveStatus({ ...online, status, observation_status: "stale", observation_expires_at: null }, receivedAt), "stale");
    assert.equal(effectiveStatus({ ...online, device_status: status, observation_status: "stale" }, receivedAt), "stale");
  }
  assert.equal(effectiveStatus({ ...online, observation_status: "unknown", last_observed_at: null, observation_expires_at: null }, receivedAt), "unknown");
  assert.equal(effectiveStatus({ ...online, enabled: false, observation_status: "stale" }, receivedAt), "disabled");
  assert.equal(effectiveStatus({ ...online, status: "offline" }, receivedAt), "offline");
});

test("an unchanged browser snapshot ages out at its server-issued expiry", () => {
  assert.equal(effectiveStatus(online, receivedAt + 29_999), "online");
  assert.equal(effectiveStatus(online, receivedAt + 30_000), "stale");
  assert.equal(effectiveStatus(online, receivedAt + 31_000), "stale");
  assert.equal(online.status, "online", "the last known state is preserved");
});

test("camera responses require the current observation contract", () => {
  assert.equal(isCameraObservation(onlineResponse), true);
  assert.equal(isCameraObservation({ ...onlineResponse, observation_status: "stale", observation_expires_at: null, observation_remaining_ms: 0 }), true);
  assert.equal(isCameraObservation({ observation_status: "unknown", last_observed_at: null, observation_expires_at: null, observation_remaining_ms: 0 }), true);
  assert.equal(isCameraObservation({ ...onlineResponse, observation_remaining_ms: 0 }), true, "sub-millisecond freshness can round down to zero");
  for (const value of [{}, { ...onlineResponse, observation_status: "online" }, { ...onlineResponse, last_observed_at: null }, { ...onlineResponse, last_observed_at: "bad" }, { ...onlineResponse, observation_expires_at: null }, { ...onlineResponse, observation_expires_at: observed }, { ...onlineResponse, observation_status: "stale" }]) {
    assert.equal(isCameraObservation(value), false);
  }
  for (const remaining of [undefined, null, "30000", -1, 0.5, NaN, Infinity, Number.MAX_SAFE_INTEGER + 1]) {
    assert.equal(isCameraObservation({ ...onlineResponse, observation_remaining_ms: remaining }), false);
  }
});

test("browser clock skew and wall-clock adjustments cannot change observation freshness", t => {
  let monotonicNow = receivedAt;
  let wallNow = Date.parse(observed);
  t.mock.method(performance, "now", () => monotonicNow);
  t.mock.method(Date, "now", () => wallNow);
  for (const offset of [0, 120_000, -120_000, 86_400_000, -86_400_000]) {
    wallNow = Date.parse(observed) + offset;
    assert.equal(effectiveStatus(online), "online");
    assert.equal(remainingObservationMs(online), 30_000);
  }
  monotonicNow += 30_000;
  assert.equal(effectiveStatus(online), "stale");
  wallNow -= 86_400_000;
  assert.equal(effectiveStatus(online), "stale", "clock rollback cannot revive an expired snapshot");
});

test("network delay consumes validity instead of extending it at receipt", () => {
  assert.equal(remainingObservationMs(online, receivedAt + 10_000), 20_000);
  assert.equal(effectiveStatus(online, receivedAt + 30_000), "stale");
  const expiredInTransit = cameraSnapshot({ ...onlineResponse, observation_remaining_ms: 50 }, receivedAt);
  assert.equal(effectiveStatus(expiredInTransit, receivedAt + 100), "stale");
  assert.equal(remainingObservationMs(expiredInTransit, receivedAt + 100), 0);
});

function statusHost(t, camera = online) {
  let now = receivedAt;
  const timers = new Map();
  let nextTimer = 0;
  t.mock.method(performance, "now", () => now);
  t.mock.method(window, "setTimeout", (fn, delay) => {
    timers.set(++nextTimer, { fn, delay });
    return nextTimer;
  });
  t.mock.method(window, "clearTimeout", id => timers.delete(id));
  const host = new HookHost(CameraStatus, { camera });
  t.after(() => host.unmount());
  const advance = value => {
    now = value;
    const [id, timer] = [...timers.entries()][0];
    timers.delete(id);
    timer.fn();
    return textContent(host.render());
  };
  return { host, timers, advance, setNow: value => { now = value; } };
}

test("the real badge ages out without a refresh and keeps its deadline across rerenders", t => {
  const { host, timers, advance } = statusHost(t);
  assert.equal(textContent(host.render()), "Online");
  const originalTimer = [...timers.keys()][0];
  assert.equal([...timers.values()][0].delay, 30_001);
  t.mock.method(Date, "now", () => Date.parse(expires) + 120_000);
  assert.equal(textContent(host.render()), "Online");
  assert.equal([...timers.keys()][0], originalTimer, "rerendering does not renew the observation");
  assert.equal(advance(receivedAt + 30_000), "Observation stale");
  assert.equal(timers.size, 0);
});

test("a timer that fires before the monotonic deadline reschedules the remaining time", t => {
  const { host, timers, advance } = statusHost(t);
  host.render();
  assert.equal(advance(receivedAt + 29_999), "Online");
  assert.equal([...timers.values()][0].delay, 2);
  assert.equal(advance(receivedAt + 30_000), "Observation stale");
  assert.equal(timers.size, 0);
});

test("a refreshed snapshot replaces the timer even when the absolute server expiry is unchanged", t => {
  const { host, timers, advance, setNow } = statusHost(t);
  host.render();
  const oldTimer = [...timers.keys()][0];
  setNow(receivedAt + 1_000);
  host.props.camera = cameraSnapshot({ ...onlineResponse, observation_remaining_ms: 29_000 }, receivedAt + 900);
  assert.equal(textContent(host.render()), "Online");
  assert.equal(timers.has(oldTimer), false);
  assert.equal([...timers.values()][0].delay, 28_901);
  assert.equal(advance(receivedAt + 29_900), "Observation stale");
  host.props.camera = cameraSnapshot({ ...onlineResponse, observation_expires_at: "2026-10-10T11:02:00Z", observation_remaining_ms: 30_000 }, receivedAt + 29_900);
  assert.equal(textContent(host.render()), "Online", "a new observation can restore freshness");
  host.unmount();
  assert.equal(timers.size, 0, "unmount cancels the replacement timer");
});

test("zero remaining validity never schedules a fresh badge", t => {
  const expired = cameraSnapshot({ ...onlineResponse, observation_remaining_ms: 0 }, receivedAt);
  const { host, timers } = statusHost(t, expired);
  assert.equal(textContent(host.render()), "Observation stale");
  assert.equal(timers.size, 0);
});

test("expiry between render and effect immediately updates the badge", t => {
  const { host, timers, setNow } = statusHost(t);
  const tree = host.render(() => setNow(receivedAt + 30_000));
  assert.equal(textContent(tree), "Observation stale");
  assert.equal(timers.size, 0);
});
