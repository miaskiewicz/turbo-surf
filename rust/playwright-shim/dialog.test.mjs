import assert from "node:assert/strict";
import test from "node:test";
import { dialogMessage, dialogReplyUnits } from "./dialog.mjs";

test("dialog payload identifies its actual Page and normalizes omitted prompt replacement", async () => {
  const page = {};
  const replies = [];
  const dialog = dialogMessage(
    page,
    { id: "12", type: "prompt", message: "message", defaultValue: "initial" },
    (reply) => replies.push(reply),
  );
  assert.equal(dialog.page(), page);
  assert.equal(dialog.type(), "prompt");
  assert.equal(dialog.message(), "message");
  assert.equal(dialog.defaultValue(), "initial");
  await dialog.accept();
  assert.deepEqual(replies, [{ id: "12", accepted: true, promptText: "" }]);
  await assert.rejects(dialog.dismiss(), /already handled/);
  assert.equal(replies.length, 1);
});

test("an explicit empty prompt replacement is sent unchanged", async () => {
  const replies = [];
  const dialog = dialogMessage(
    {},
    { id: "13", type: "prompt", message: "", defaultValue: "initial" },
    (reply) => replies.push(reply),
  );
  await dialog.accept("");
  assert.deepEqual(replies, [{ id: "13", accepted: true, promptText: "" }]);
});

test("invalid prompt replacement leaves the dialog available for dismissal", async () => {
  const replies = [];
  const dialog = dialogMessage(
    {},
    { id: "14", type: "prompt", message: "", defaultValue: "" },
    (reply) => replies.push(reply),
  );
  await assert.rejects(dialog.accept(12), /must be a string/);
  await dialog.dismiss();
  assert.deepEqual(replies, [{ id: "14", accepted: false, promptText: undefined }]);
});

test("native reply errors propagate and cannot be retried as a second handling", async () => {
  const dialog = dialogMessage(
    {},
    { id: "15", type: "alert", message: "", defaultValue: "" },
    () => {
      throw new Error("Target closed");
    },
  );
  await assert.rejects(dialog.accept(), /Target closed/);
  await assert.rejects(dialog.accept(), /already handled/);
});

test("native UTF-16 transport preserves lone surrogates, pairs, empty and omitted text", () => {
  const units = [0xd800, 0, 0xdc00, 0xd83d, 0xde00];
  const dialog = dialogMessage({}, { messageUnits: units, defaultValueUnits: [0xdc00] }, () => {});
  assert.deepEqual(dialogReplyUnits(dialog.message()), units);
  assert.deepEqual(dialogReplyUnits(dialog.defaultValue()), [0xdc00]);
  assert.deepEqual(dialogReplyUnits(""), []);
  assert.equal(dialogReplyUnits(undefined), undefined);
});
