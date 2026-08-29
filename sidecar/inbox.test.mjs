/** The streaming-input inbox: the thing that makes a mid-turn message possible.
 *
 *  What matters is not that a message goes in, but *when* it is called read.
 *  The ack rides on the `yield` coming back — the moment the SDK has written
 *  the message to the CLI — and a test that only checked the queue would pass
 *  while the screen lied about what the model had seen. So this drives the
 *  generator the way `streamInput` does, one pull at a time. */

import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const source = fs.readFileSync(path.join(here, "index.mjs"), "utf8");

/** index.mjs starts a readline loop on import, so only the two definitions
 *  under test are evaluated — same trick as mcp-wiring.test.mjs. */
function load() {
  const start = source.indexOf("function userMessage(");
  const end = source.indexOf("function summarize(");
  assert.ok(start > -1 && end > start, "the inbox moved; update this test");
  const sent = [];
  const factory = new Function(
    "__send",
    `const send = __send;
     ${source.slice(start, end)}
     return { Inbox, userMessage };`,
  );
  return { ...factory((m) => sent.push(m)), sent };
}

test("a message is shaped exactly as the SDK shapes a string prompt", () => {
  const { userMessage } = load();
  assert.deepEqual(userMessage("hello"), {
    type: "user",
    session_id: "",
    message: { role: "user", content: [{ type: "text", text: "hello" }] },
    parent_tool_use_id: null,
  });
});

test("the prompt is the first thing the SDK reads", async () => {
  const { Inbox } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("do the thing", "run-1")[Symbol.asyncIterator]();
  const first = await stream.next();
  assert.equal(first.value.message.content[0].text, "do the thing");
});

test("a message pushed mid-turn is read without the turn ending", async () => {
  const { Inbox, sent } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("first", "run-1")[Symbol.asyncIterator]();
  await stream.next();

  inbox.push({ id: "q1", text: "actually, only the sidecar tests" });
  const queued = await stream.next();
  assert.equal(queued.value.message.content[0].text, "actually, only the sidecar tests");
  // Written, not yet acknowledged: the ack rides on the *next* pull, which is
  // when `streamInput` has finished writing this one.
  assert.equal(sent.length, 0, "nothing is called read before it is written");
});

test("read is announced only once the SDK has taken the message", async () => {
  const { Inbox, sent } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("first", "run-7")[Symbol.asyncIterator]();
  await stream.next();
  inbox.push({ id: "q1", text: "queued" });
  await stream.next();

  const pulling = stream.next();
  await Promise.resolve();
  assert.deepEqual(sent, [
    { type: "event", run_id: "run-7", event: { kind: "message_read", message_id: "q1" } },
  ]);

  inbox.close();
  assert.equal((await pulling).done, true);
});

test("two messages queued quickly arrive in order", async () => {
  const { Inbox } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("first", "run-1")[Symbol.asyncIterator]();
  await stream.next();

  inbox.push({ id: "q1", text: "one" });
  inbox.push({ id: "q2", text: "two" });

  assert.equal((await stream.next()).value.message.content[0].text, "one");
  assert.equal((await stream.next()).value.message.content[0].text, "two");
});

test("closing ends the stream, which is what closes the CLI's stdin", async () => {
  const { Inbox } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("first", "run-1")[Symbol.asyncIterator]();
  await stream.next();

  const pulling = stream.next();
  inbox.close();
  assert.equal((await pulling).done, true);
});

test("a message pushed after the run ended is refused rather than swallowed", async () => {
  const { Inbox } = load();
  const inbox = new Inbox();
  inbox.close();
  assert.equal(inbox.push({ id: "q1", text: "too late" }), false);
});

test("closing discards rather than draining into a dead run", async () => {
  const { Inbox, sent } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("first", "run-1")[Symbol.asyncIterator]();
  await stream.next();
  inbox.push({ id: "q1", text: "in time" });
  // Handing this over now would write it to a CLI that is about to be killed,
  // and call it read while nothing ever answered it. Unacknowledged, it goes
  // back to the Rust side and becomes a turn of its own.
  inbox.close();
  assert.equal((await stream.next()).done, true);
  assert.equal(sent.length, 0, "nothing was called read");
});

test("the run is owed a result for every message it was handed", async () => {
  const { Inbox } = load();
  const inbox = new Inbox();
  const stream = inbox.stream("first", "run-1")[Symbol.asyncIterator]();
  await stream.next();

  // One message in, one result owed: the prompt's.
  assert.equal(inbox.owed(0), true);
  assert.equal(inbox.owed(1), false, "an ordinary turn ends on its first result");

  inbox.push({ id: "q1", text: "and one more thing" });
  // Pushed and not yet written: still owed, or the run would end between the
  // push and the hand-over.
  assert.equal(inbox.owed(1), true);
  await stream.next();
  // Written: the CLI answers it with a second result of its own, which is the
  // turn that reads what was queued.
  assert.equal(inbox.owed(1), true);
  assert.equal(inbox.owed(2), false);
});
