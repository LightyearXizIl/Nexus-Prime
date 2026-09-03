import { describe, expect, it } from "vitest";
import { batteryTone, displayBatteryLevel } from "./battery";

describe("local battery state", () => {
  it("uses the requested threshold colors", () => {
    expect(batteryTone(30)).toBe("green");
    expect(batteryTone(29)).toBe("yellow");
    expect(batteryTone(10)).toBe("yellow");
    expect(batteryTone(9)).toBe("red");
    expect(batteryTone(null)).toBe("unknown");
  });

  it("keeps an unknown battery empty", () => {
    expect(displayBatteryLevel(undefined)).toBeNull();
    expect(displayBatteryLevel(-1)).toBe(0);
    expect(displayBatteryLevel(101)).toBe(100);
  });
});
