import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant";

async function instrumentDataPublishing(page) {
  await page.evaluate(() => {
    globalThis.__dataPublishCalls = 0;
    const prototype = globalThis.LivekitClient.LocalParticipant.prototype;
    const original = prototype.publishData;
    prototype.publishData = function (...args) {
      globalThis.__dataPublishCalls += 1;
      return original.apply(this, args);
    };
  });
}

test("reliable and lossy native byte packets remain independent", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `data-packets-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "packet-sender",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "packet-receiver",
  });

  try {
    const received = receiver.page.locator("#data-packets");
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await expect(received).toHaveAttribute("data-observing", "true");

    await sender.page.locator("#send-reliable-data-packet").dispatchEvent("click");

    await expect(received).toHaveAttribute("data-reliable-count", "1");
    await expect(received).toHaveAttribute("data-reliable-sender", "packet-sender");
    await expect(received).toHaveAttribute("data-reliable-payload", "0,255,128,65");
    await expect(received).toHaveAttribute("data-lossy-count", "0");

    await sender.page.locator("#abandon-reliable-data-packet-send").dispatchEvent("click");

    await expect(received).toHaveAttribute("data-reliable-count", "2");
    await expect(received).toHaveAttribute("data-reliable-payload", "9,0,253,67");

    for (let attempt = 0; attempt < 3; attempt += 1) {
      await sender.page.locator("#send-lossy-data-packet").dispatchEvent("click");
    }

    await expect
      .poll(async () => Number(await received.getAttribute("data-lossy-count")))
      .toBeGreaterThan(0);
    await expect(received).toHaveAttribute("data-lossy-sender", "packet-sender");
    await expect(received).toHaveAttribute("data-lossy-payload", "7,0,254,66");
    await expect(received).toHaveAttribute("data-reliable-count", "2");
    await expect(received).toHaveAttribute("data-dropped", "0");

    await Promise.all([
      sender.page.getByRole("button", { name: "Leave room" }).click(),
      receiver.page.getByRole("button", { name: "Leave room" }).click(),
    ]);
    await expect(sender.page.locator("#data-packets")).toHaveAttribute("data-closed", "true");
    await expect(received).toHaveAttribute("data-closed", "true");
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});

test("data publishing denial remains typed and sends nothing", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `data-packets-denied-${browserName}-${Date.now()}`;
  const denied = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "data-packets-denied",
    prepare: instrumentDataPublishing,
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "packet-observer",
  });

  try {
    await expect(denied.page.locator("#room-session")).toHaveAttribute(
      "data-can-publish-data",
      "false",
    );

    await denied.page.locator("#send-reliable-data-packet").dispatchEvent("click");

    await expect(denied.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    await expect(denied.page.locator(".notice.error")).toContainText(
      "LiveKit source grants do not permit publishing data",
    );
    expect(await denied.page.evaluate(() => globalThis.__dataPublishCalls)).toBe(0);
    await expect(receiver.page.locator("#data-packets")).toHaveAttribute(
      "data-reliable-count",
      "0",
    );
  } finally {
    await Promise.all([denied.context.close(), receiver.context.close()]);
  }
});
