import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant.mjs";

const TEXT_CHUNKS = 128;
const BYTE_CHUNKS = 128;
const BYTE_CHUNK_SIZE = 12_000;
const textChunk = "LiveKit text stream λ|".repeat(500);
const textChunkBytes = new TextEncoder().encode(textChunk);
const expectedTextBytes = textChunkBytes.length * TEXT_CHUNKS;
const expectedTextChecksum =
  textChunkBytes.reduce((sum, byte) => sum + byte, 0) * TEXT_CHUNKS;
const expectedByteBytes = BYTE_CHUNKS * BYTE_CHUNK_SIZE;
let expectedByteChecksum = 0;
for (let index = 0; index < BYTE_CHUNKS; index += 1) {
  for (let offset = 0; offset < BYTE_CHUNK_SIZE; offset += 1) {
    expectedByteChecksum += (index + offset) % 251;
  }
}

async function instrumentDataStreamOpening(page) {
  await page.evaluate(() => {
    const prototype = globalThis.LivekitClient.LocalParticipant.prototype;
    const original = prototype.streamText;
    globalThis.__textStreamOpenCalls = 0;
    prototype.streamText = function (...args) {
      globalThis.__textStreamOpenCalls += 1;
      return original.apply(this, args);
    };
  });
}

async function failFirstTextWrite(page) {
  await page.evaluate(() => {
    const prototype = globalThis.LivekitClient.LocalParticipant.prototype;
    const original = prototype.streamText;
    globalThis.__streamWriterReleaseCalls = 0;
    prototype.streamText = async function (...args) {
      const writer = await original.apply(this, args);
      const originalRelease = writer.defaultWriter.releaseLock.bind(writer.defaultWriter);
      writer.defaultWriter.releaseLock = () => {
        globalThis.__streamWriterReleaseCalls += 1;
        return originalRelease();
      };
      await writer.defaultWriter.abort(new Error("injected stream sink failure"));
      return writer;
    };
  });
}

async function stallFirstTextWrite(page) {
  await page.evaluate(() => {
    const prototype = globalThis.LivekitClient.LocalParticipant.prototype;
    const original = prototype.streamText;
    globalThis.__stalledTextWriteCalls = 0;
    globalThis.__stalledTextWriteGate = new Promise((resolve) => {
      globalThis.__releaseStalledTextWrite = resolve;
    });
    prototype.streamText = async function (...args) {
      const writer = await original.apply(this, args);
      const originalWrite = writer.write.bind(writer);
      writer.write = async (chunk) => {
        globalThis.__stalledTextWriteCalls += 1;
        await globalThis.__stalledTextWriteGate;
        return originalWrite(chunk);
      };
      return writer;
    };
  });
}

test("large text and byte streams preserve payloads and sender backpressure", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const room = `data-streams-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "stream-sender",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "stream-receiver",
  });

  try {
    const sent = sender.page.locator("#data-streams");
    const received = receiver.page.locator("#data-streams");
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await expect(received).toHaveAttribute("data-text-handler-active", "true");
    await expect(received).toHaveAttribute("data-byte-handler-active", "true");

    await receiver.page.locator("#pause-data-stream-reads").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-reads-paused", "true");
    await sender.page.locator("#send-large-text-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-text-sender", "stream-sender");
    await sender.page.waitForTimeout(250);
    await expect(sent).toHaveAttribute("data-sent-text-writes", "0");
    await expect(sent).toHaveAttribute("data-sent-text-completed", "false");

    await receiver.page.locator("#resume-data-stream-reads").dispatchEvent("click");
    await expect(sent).toHaveAttribute("data-sent-text-completed", "true", {
      timeout: 20_000,
    });
    await expect(sent).toHaveAttribute("data-sent-text-writes", String(TEXT_CHUNKS));
    await expect(received).toHaveAttribute("data-text-completed", "true");
    await expect(received).toHaveAttribute("data-text-sender", "stream-sender");
    await expect(received).toHaveAttribute("data-text-bytes", String(expectedTextBytes));
    await expect(received).toHaveAttribute(
      "data-text-checksum",
      String(expectedTextChecksum),
    );
    await expect(received).toHaveAttribute("data-text-error", "");

    await receiver.page.locator("#pause-data-stream-reads").dispatchEvent("click");
    await sender.page.locator("#send-large-byte-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-byte-sender", "stream-sender");
    await sender.page.waitForTimeout(250);
    await expect(sent).toHaveAttribute("data-sent-byte-writes", "0");
    await expect(sent).toHaveAttribute("data-sent-byte-completed", "false");
    await receiver.page.locator("#resume-data-stream-reads").dispatchEvent("click");
    await expect(sent).toHaveAttribute("data-sent-byte-completed", "true", {
      timeout: 20_000,
    });
    await expect(sent).toHaveAttribute("data-sent-byte-writes", String(BYTE_CHUNKS));
    await expect(received).toHaveAttribute("data-byte-completed", "true");
    await expect(received).toHaveAttribute("data-byte-sender", "stream-sender");
    await expect(received).toHaveAttribute("data-byte-bytes", String(expectedByteBytes));
    await expect(received).toHaveAttribute(
      "data-byte-checksum",
      String(expectedByteChecksum),
    );
    await expect(received).toHaveAttribute("data-byte-error", "");
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});

test("incoming cancellation releases the SDK reader and stops the sender", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(60_000);
  const room = `data-stream-cancel-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "cancel-sender",
    prepare: stallFirstTextWrite,
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "cancel-receiver",
  });

  try {
    const sent = sender.page.locator("#data-streams");
    const received = receiver.page.locator("#data-streams");
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await sender.page.locator("#send-large-text-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-text-sender", "cancel-sender");
    await expect(received).toHaveAttribute("data-text-read-pending", "true");
    await expect.poll(() => sender.page.evaluate(() => globalThis.__stalledTextWriteCalls)).toBe(1);
    await expect(sent).toHaveAttribute("data-sent-text-writes", "0");
    await receiver.page.locator("#cancel-incoming-text-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-text-cancelled", "true");
    await expect(received).toHaveAttribute("data-text-completed", "false");
    await expect(sent).toHaveAttribute("data-sent-text-completed", "false");
    await expect(sender.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "dataStreamWriteFailed",
    );
    await sender.page.evaluate(() => globalThis.__releaseStalledTextWrite());
    await sender.page.waitForTimeout(100);

    await receiver.page.locator("#pause-data-stream-reads").dispatchEvent("click");
    await sender.page.locator("#send-large-byte-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-byte-sender", "cancel-sender");
    await expect(sent).toHaveAttribute("data-sent-byte-writes", "0");
    await receiver.page.locator("#cancel-incoming-byte-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-byte-cancelled", "true");
    await expect(received).toHaveAttribute("data-byte-completed", "false");
    await expect(sent).toHaveAttribute("data-sent-byte-completed", "false");
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});

test("Remote Participant departure terminates an incomplete stream", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(60_000);
  const room = `data-stream-close-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "departing-sender",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "remaining-receiver",
  });

  try {
    const received = receiver.page.locator("#data-streams");
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await receiver.page.locator("#pause-data-stream-reads").dispatchEvent("click");
    await sender.page.locator("#send-large-text-stream").dispatchEvent("click");
    await expect(received).toHaveAttribute("data-text-sender", "departing-sender");
    await expect(sender.page.locator("#data-streams")).toHaveAttribute(
      "data-sent-text-writes",
      "0",
    );
    await sender.page.getByRole("button", { name: "Leave room" }).click();
    await expect(sender.page.locator("#connection-state")).toHaveText("disconnected");
    await receiver.page.locator("#resume-data-stream-reads").dispatchEvent("click");
    await expect.poll(async () => await received.getAttribute("data-text-error")).not.toBe("");
    await expect(received).toHaveAttribute("data-text-completed", "false");
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});

test("data publishing denial opens no SDK stream", async ({ browser, browserName, baseURL }) => {
  const room = `data-stream-denied-${browserName}-${Date.now()}`;
  const denied = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "data-streams-denied",
    prepare: instrumentDataStreamOpening,
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "data-streams-receiver",
  });

  try {
    await expect(denied.page.locator("#participant-count")).toHaveText("2");
    await denied.page.locator("#send-large-text-stream").dispatchEvent("click");
    await expect(denied.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    expect(await denied.page.evaluate(() => globalThis.__textStreamOpenCalls)).toBe(0);
  } finally {
    await Promise.all([denied.context.close(), receiver.context.close()]);
  }
});

test("terminal sink failures release the SDK writer", async ({ browser, browserName, baseURL }) => {
  const room = `data-stream-failure-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "failure-sender",
    prepare: failFirstTextWrite,
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "failure-receiver",
  });

  try {
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await sender.page.locator("#send-large-text-stream").dispatchEvent("click");
    await expect(sender.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "dataStreamWriteFailed",
    );
    await expect
      .poll(() => sender.page.evaluate(() => globalThis.__streamWriterReleaseCalls))
      .toBe(1);
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});
