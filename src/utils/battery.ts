export type BatteryTone = "green" | "yellow" | "red" | "unknown";

/** Normalizes BLE battery values for the current local battery shell. */
export function displayBatteryLevel(level: number | null | undefined): number | null {
  if (level == null || !Number.isFinite(level)) return null;
  return Math.max(0, Math.min(100, Math.round(level)));
}

export function batteryTone(level: number | null | undefined): BatteryTone {
  const value = displayBatteryLevel(level);
  if (value == null) return "unknown";
  if (value < 10) return "red";
  if (value < 30) return "yellow";
  return "green";
}
