import { describe, expect, it } from "vitest";
import { createMockBackend } from "@/backend/mock";
import { angleAt, arcPath, lidConfigValid, sensitivityFactor, sensitivityPos, waveY, withSensitivity } from "./sensors";

const lid = createMockBackend().data.config.lid;
const slap = createMockBackend().data.config.slap;

describe("lidConfigValid", () => {
  it("mirrors LidConfig::validate", () => {
    expect(lidConfigValid(lid)).toBe(true);
    expect(lidConfigValid({ ...lid, close_threshold_deg: 130 })).toBe(false);
    expect(lidConfigValid({ ...lid, hysteresis_deg: 0 })).toBe(false);
    expect(lidConfigValid({ ...lid, close_threshold_deg: 100, overbend_threshold_deg: 109 })).toBe(false);
    expect(lidConfigValid({ ...lid, open_min_deg: 181 })).toBe(false);
  });
});

describe("gauge geometry", () => {
  it("turns pointer positions into angles", () => {
    expect(angleAt(170, 166, 300, 166)).toBe(0);
    expect(angleAt(170, 166, 170, 20)).toBe(90);
    expect(angleAt(170, 166, 40, 166)).toBe(180);
    expect(angleAt(170, 166, 40, 200)).toBe(180); // below the hinge on the left
    expect(angleAt(170, 166, 300, 200)).toBe(0);
  });

  it("draws arcs counter-clockwise", () => {
    expect(arcPath(0, 0, 10, 0, 90)).toBe("M 10.0 0.0 A 10 10 0 0 0 0.0 -10.0");
  });

  it("maps g to a clamped y", () => {
    expect(waveY(0, 96)).toBe("92.0");
    expect(waveY(1, 96)).toBe("8.0");
    expect(waveY(5, 96)).toBe("8.0");
  });
});

describe("sensitivity", () => {
  const base = { light_g: 0.2, medium_g: 0.4, hard_g: 0.8 };

  it("is 50 at the calibrated levels and 0–100 for 0.5×–2×", () => {
    expect(sensitivityPos(base, base)).toBe(50);
    expect(sensitivityPos(base, { light_g: 0.1, medium_g: 0.2, hard_g: 0.4 })).toBe(100);
    expect(sensitivityPos(base, { light_g: 0.4, medium_g: 0.8, hard_g: 1.6 })).toBe(0);
    expect(sensitivityFactor(50)).toBe(1);
    expect(sensitivityFactor(100)).toBe(2);
  });

  it("scales every level and keeps the trigger at or below light", () => {
    const easy = withSensitivity(slap, base, 0.15, 100);
    expect(easy.levels).toEqual({ light_g: 0.1, medium_g: 0.2, hard_g: 0.4 });
    expect(easy.trigger_g).toBe(0.1);
    const hard = withSensitivity(slap, base, 0.15, 0);
    expect(hard.levels.light_g).toBe(0.4);
    expect(hard.trigger_g).toBe(0.15);
    expect(sensitivityPos(base, hard.levels)).toBe(0);
  });
});
