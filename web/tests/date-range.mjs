import assert from "node:assert/strict";
import { expect } from "@playwright/test";
export function rangeEditor(page, id) {
  const root = page.locator(`#${id}`);
  const fields = ["year", "month", "day"];
  return { root,
    async draft(start, end = start) {
      for (const [endpoint, date] of [["start", start], ["end", end]]) {
        const parts = date.split("-");
        for (const [index, field] of fields.entries()) await root.locator(`#${id}-${endpoint}-${field}`).fill(index === 0 ? parts[index] ?? "" : parts[index] ? String(Number(parts[index])) : "");
      }
    },
    async apply() { await root.locator("input").last().press("Enter"); },
    async fill(start, end = start) { await this.draft(start, end); await this.apply(); },
    async expectValue(start, end = start) {
      for (const [endpoint, date] of [["start", start], ["end", end]]) {
        for (const [index, field] of fields.entries()) await expect(root.locator(`#${id}-${endpoint}-${field}`)).toHaveValue(index === 0 ? date.split("-")[index] : String(Number(date.split("-")[index])));
      }
    },
  };
}
// SSE may refresh the last committed query while the user edits local drafts.
// Compare complete request URLs so malformed, partial or prematurely applied
// ranges cannot hide behind a legitimate background refresh.
export function assertCommittedLogRequests(requests, before) {
  const committed = new Map(before.map(path => [new URL(path, "https://test.invalid").pathname, path]));
  assert.ok(committed.size > 0, "the committed log queries must already be loaded");
  for (const path of requests.slice(before.length)) {
    assert.equal(path, committed.get(new URL(path, "https://test.invalid").pathname), "draft edits must never change the committed log query");
  }
}
export async function checkDateRangeValidation(editor, requests, start, end) {
  const before = requests().slice();
  await editor.draft("2022-02-30", "2023-02-02");
  await expect(editor.root.locator('input[aria-invalid="true"]')).toHaveCount(1);
  await expect(editor.root.locator('input[aria-invalid="true"]')).toHaveAttribute("id", /start-day$/);
  await expect(editor.root.getByRole("alert")).toContainText("有效的年月日");
  const colors = await editor.root.evaluate(root => ({ bad: getComputedStyle(root.querySelector('[aria-invalid="true"]')).color, separators: [...root.querySelectorAll('.xcss-date-range-separator')].map(node => getComputedStyle(node).color) }));
  assert.ok(colors.separators.every(color => color !== colors.bad), "slashes and range separator keep their normal color");
  await editor.apply();
  await editor.draft("2024-13-01", "2025-01-01");
  await expect(editor.root.locator('input[aria-invalid="true"]')).toHaveCount(1);
  await expect(editor.root.locator('input[aria-invalid="true"]')).toHaveAttribute("id", /start-month$/);
  await editor.apply();
  await editor.draft("0000-01-01", "2025-01-01");
  await expect(editor.root.locator('input[aria-invalid="true"]')).toHaveCount(1);
  await editor.apply();
  await editor.draft("2023-02-02", "2022-02-01");
  await expect(editor.root.locator('input[aria-invalid="true"]')).toHaveCount(6);
  await expect(editor.root.getByRole("alert")).toContainText("结束日期不能早于");
  await editor.apply();
  assertCommittedLogRequests(requests(), before);
  await editor.draft(start, end);
  await expect(editor.root.getByRole("alert")).toHaveCount(0);
  assertCommittedLogRequests(requests(), before);
  const beforeApply = requests().length;
  await editor.apply();
  const expected = new URLSearchParams(start === end ? { date: start } : { start_date: start, end_date: end }).toString();
  const endpoints = [...new Set(before.map(path => new URL(path, "https://test.invalid").pathname))].sort();
  await expect.poll(() => [...new Set(requests().slice(beforeApply).filter(path => new URL(path, "https://test.invalid").search.slice(1) === expected).map(path => new URL(path, "https://test.invalid").pathname))].sort()).toEqual(endpoints);
}
