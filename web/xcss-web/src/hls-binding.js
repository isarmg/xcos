
// HLS's browser media engine is the only JavaScript dependency. All retries,
// tickets, lifecycle decisions and visible state are owned by the Rust player.
export async function createHls(video, url, token, manifest, failure) {
  const endpoint = new URL(url, location.href);
  if (endpoint.origin !== location.origin || endpoint.username || endpoint.password) {
    throw new Error('Media URL must use the application origin');
  }
  const { default: Hls } = await import('hls.js');
  const hls = new Hls({ lowLatencyMode: true, xhrSetup(xhr, value) {
    let request;
    try { request = new URL(value, location.href); } catch { xhr.abort(); return; }
    if (request.origin === location.origin && !request.username && !request.password) {
      xhr.setRequestHeader('Authorization', `Bearer ${token}`);
    } else xhr.abort();
  }});
  hls.on(Hls.Events.MANIFEST_PARSED, manifest);
  hls.on(Hls.Events.ERROR, (_event, data) => { if (data.fatal) failure(); });
  hls.loadSource(endpoint.href);
  hls.attachMedia(video);
  return hls;
}

export function destroyHls(hls) { hls.destroy(); }
