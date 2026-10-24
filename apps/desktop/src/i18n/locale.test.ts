import { describe, expect, it } from "vitest";
import en from "./messages/en.json";
import zhHant from "./messages/zh-Hant.json";
import { resolveLocale } from "./locale";

function keys(obj: object, prefix = ""): string[] {
  return Object.entries(obj).flatMap(([k, v]) =>
    typeof v === "object" && v !== null ? keys(v, `${prefix}${k}.`) : [`${prefix}${k}`],
  );
}

describe("resolveLocale", () => {
  it("honours an explicit setting", () => {
    expect(resolveLocale("zh-hant", ["en-US"])).toBe("zh-Hant");
    expect(resolveLocale("en", ["zh-TW"])).toBe("en");
  });

  it("follows the system language list", () => {
    expect(resolveLocale("system", ["zh-TW", "en-US"])).toBe("zh-Hant");
    expect(resolveLocale("system", ["zh-Hans-CN"])).toBe("zh-Hant");
    expect(resolveLocale("system", ["en-GB"])).toBe("en");
    expect(resolveLocale("system", ["ja-JP", "zh-TW"])).toBe("zh-Hant");
    expect(resolveLocale("system", ["ja-JP"])).toBe("en");
    expect(resolveLocale("system", [])).toBe("en");
  });
});

describe("messages", () => {
  it("zh-Hant and en have the same keys", () => {
    expect(keys(en).sort()).toEqual(keys(zhHant).sort());
  });
});
