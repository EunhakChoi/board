import assert from "node:assert/strict";
import { test } from "node:test";
import en from "../locales/en.json" with { type: "json" };
import ko from "../locales/ko.json" with { type: "json" };
import { detectLocale, t } from "./index.ts";

test("fills placeholders and falls back to English", () => {
  assert.equal(t("cli.version", { version: "0.0.0" }), "board 0.0.0");
  assert.equal(t("cli.usage", {}, "ko"), "사용법: board <명령> [옵션]");
});

test("detects Korean from the environment, English otherwise", () => {
  assert.equal(detectLocale({ LANG: "ko_KR.UTF-8" }), "ko");
  assert.equal(detectLocale({ LANG: "C.UTF-8" }), "en");
  assert.equal(detectLocale({ BOARD_LANG: "ko", LANG: "en_US" }), "ko");
});

test("every Korean key exists in English", () => {
  for (const key of Object.keys(ko)) assert.ok(key in en, `missing in en: ${key}`);
});
