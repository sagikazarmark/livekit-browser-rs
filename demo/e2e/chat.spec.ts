import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant";

async function editFirstChatMessage(page) {
  await page.evaluate(() => {
    const prototype = globalThis.LivekitClient.LocalParticipant.prototype;
    const original = prototype.sendChatMessage;
    prototype.sendChatMessage = async function (text, ...args) {
      const message = await original.call(this, text, ...args);
      if (text === "first chat λ") {
        await this.editChatMessage("edited first chat λ", message);
      }
      return message;
    };
  });
}

test("chat messages preserve order, sender identity, timing, and cleanup", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `chat-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "chat-sender",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "chat-receiver",
  });

  try {
    const senderMessages = sender.page.locator("#chat-messages");
    const received = receiver.page.locator("#chat-messages");
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await expect(senderMessages).toHaveAttribute("data-observing", "true");
    await expect(received).toHaveAttribute("data-observing", "true");

    await sender.page.locator("#observe-room-events").dispatchEvent("click");
    await receiver.page.locator("#observe-room-events").dispatchEvent("click");
    await expect(sender.page.locator("#room-events")).toHaveAttribute("data-observing", "true");
    await expect(receiver.page.locator("#room-events")).toHaveAttribute("data-observing", "true");
    const senderEventCount = await sender.page.locator("#room-events").getAttribute("data-count");
    const receiverEventCount = await receiver.page
      .locator("#room-events")
      .getAttribute("data-count");

    await sender.page.locator("#send-chat-messages").dispatchEvent("click");

    for (const chat of [senderMessages, received]) {
      await expect(chat).toHaveAttribute("data-count", "3");
      await expect(chat).toHaveAttribute(
        "data-contents",
        "first chat λ|second chat|third chat",
      );
      await expect(chat).toHaveAttribute(
        "data-senders",
        "chat-sender|chat-sender|chat-sender",
      );
      await expect(chat).toHaveAttribute("data-dropped", "0");
    }

    const ids = (await received.getAttribute("data-ids")).split("|");
    expect(ids).toHaveLength(3);
    expect(ids.every(Boolean)).toBe(true);
    expect(new Set(ids).size).toBe(3);
    const timestamps = (await received.getAttribute("data-timestamps"))
      .split("|")
      .map(Number);
    expect(timestamps).toHaveLength(3);
    expect(timestamps.every(Number.isSafeInteger)).toBe(true);
    expect(timestamps.every((timestamp) => timestamp > 0)).toBe(true);
    expect(timestamps[0]).toBeLessThanOrEqual(timestamps[1]);
    expect(timestamps[1]).toBeLessThanOrEqual(timestamps[2]);
    await expect(senderMessages).toHaveAttribute("data-sent-count", "3");
    await expect(senderMessages).toHaveAttribute("data-send-error", "");

    expect(await sender.page.locator("#room-events").getAttribute("data-count")).toBe(
      senderEventCount,
    );
    expect(await receiver.page.locator("#room-events").getAttribute("data-count")).toBe(
      receiverEventCount,
    );

    await Promise.all([
      sender.page.getByRole("button", { name: "Leave room" }).click(),
      receiver.page.getByRole("button", { name: "Leave room" }).click(),
    ]);
    await expect(senderMessages).toHaveAttribute("data-closed", "true");
    await expect(received).toHaveAttribute("data-closed", "true");
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});

test("received chat edits preserve LiveKit edit timing", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `chat-edit-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "chat-editor",
    prepare: editFirstChatMessage,
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "chat-edit-receiver",
  });

  try {
    const received = receiver.page.locator("#chat-messages");
    await expect(sender.page.locator("#participant-count")).toHaveText("2");
    await sender.page.locator("#send-chat-messages").dispatchEvent("click");

    await expect(received).toHaveAttribute("data-count", "4");
    await expect(received).toHaveAttribute(
      "data-contents",
      "first chat λ|edited first chat λ|second chat|third chat",
    );
    const created = (await received.getAttribute("data-timestamps")).split("|").map(Number);
    const edited = (await received.getAttribute("data-edit-timestamps")).split("|");
    expect(edited).toHaveLength(4);
    expect(edited[0]).toBe("");
    expect(Number(edited[1])).toBeGreaterThanOrEqual(created[1]);
    expect(Number(edited[1])).toBeGreaterThan(0);
    expect(edited.slice(2)).toEqual(["", ""]);
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});
