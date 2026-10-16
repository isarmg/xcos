export type CameraObservation = {
  observation_status: "fresh" | "stale" | "unknown";
  last_observed_at: string | null;
  observation_expires_at: string | null;
  observation_remaining_ms: number;
};

export type CameraSnapshot<T extends CameraObservation> = T & {
  observationDeadline: number;
};

type CameraStatus = CameraSnapshot<CameraObservation & {
  enabled: boolean;
  status: string;
  device_status: string;
}>;

const isTimestamp = (value: unknown): value is string =>
  typeof value === "string" && Number.isFinite(Date.parse(value));

export function isCameraObservation(value: Record<string, unknown>): boolean {
  const observed = value.last_observed_at;
  const expires = value.observation_expires_at;
  const remaining = value.observation_remaining_ms;
  if (typeof remaining !== "number" || !Number.isSafeInteger(remaining) || remaining < 0) return false;
  if (observed === null) return value.observation_status === "unknown" && expires === null && remaining === 0;
  if (!isTimestamp(observed)) return false;
  if (expires !== null && (!isTimestamp(expires) || Date.parse(expires) <= Date.parse(observed))) return false;
  return (value.observation_status === "stale" && remaining === 0) || (value.observation_status === "fresh" && expires !== null);
}

export function cameraSnapshot<T extends CameraObservation>(camera: T, requestStartedAt: number): CameraSnapshot<T> {
  // Anchor to the request start so transport time cannot extend the server's remaining validity.
  return { ...camera, observationDeadline: requestStartedAt + camera.observation_remaining_ms };
}

export function remainingObservationMs(camera: CameraSnapshot<CameraObservation>, now = performance.now()): number {
  return camera.observation_status === "fresh" ? Math.max(0, camera.observationDeadline - now) : 0;
}

export function effectiveStatus(camera: CameraStatus, now = performance.now()): string {
  if (!camera.enabled) return "disabled";
  if (camera.observation_status !== "fresh") return camera.observation_status;
  if (camera.observation_expires_at === null || remainingObservationMs(camera, now) <= 0) return "stale";
  return camera.device_status === "online" ? camera.status : camera.device_status;
}
