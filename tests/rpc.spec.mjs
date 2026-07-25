import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant.mjs";

async function instrumentRpcRegistration(page) {
  await page.evaluate(() => {
    const prototype = globalThis.LivekitClient.Room.prototype;
    const register = prototype.registerRpcMethod;
    const unregister = prototype.unregisterRpcMethod;
    globalThis.__rpcRegistrations = 0;
    globalThis.__rpcUnregistrations = 0;
    prototype.registerRpcMethod = function (...args) {
      globalThis.__rpcRegistrations += 1;
      return register.apply(this, args);
    };
    prototype.unregisterRpcMethod = function (...args) {
      globalThis.__rpcUnregistrations += 1;
      return unregister.apply(this, args);
    };
  });
}

test("Participant RPC exchanges text and bytes without entering Room Events", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `rpc-success-${browserName}-${Date.now()}`;
  const caller = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "rpc-caller",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "rpc-receiver",
  });

  try {
    const callerRpc = caller.page.locator("#participant-rpc");
    const receiverRpc = receiver.page.locator("#participant-rpc");
    await expect(caller.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await expect(receiverRpc).toHaveAttribute("data-echo-registered", "true");
    await expect(receiverRpc).toHaveAttribute("data-method-count", "3");

    await caller.page.locator("#observe-room-events").dispatchEvent("click");
    await receiver.page.locator("#observe-room-events").dispatchEvent("click");
    await expect(caller.page.locator("#room-events")).toHaveAttribute("data-observing", "true");
    await expect(receiver.page.locator("#room-events")).toHaveAttribute("data-observing", "true");
    const callerEventCount = await caller.page.locator("#room-events").getAttribute("data-count");
    const receiverEventCount = await receiver.page
      .locator("#room-events")
      .getAttribute("data-count");

    await caller.page.locator("#invoke-text-rpc").dispatchEvent("click");
    await expect(callerRpc).toHaveAttribute("data-response-kind", "text");
    await expect(callerRpc).toHaveAttribute("data-response-text", "hello λ");
    await expect(receiverRpc).toHaveAttribute("data-handler-caller", "rpc-caller");
    await expect(receiverRpc).toHaveAttribute("data-handler-payload-kind", "text");
    await expect(receiverRpc).toHaveAttribute("data-handler-payload", "hello λ");
    await caller.page.locator("#invoke-byte-rpc").dispatchEvent("click");
    await expect(callerRpc).toHaveAttribute("data-response-kind", "bytes");
    await expect(callerRpc).toHaveAttribute("data-response-bytes", "0,255,128,65");
    await expect(receiverRpc).toHaveAttribute("data-handler-payload-kind", "bytes");
    await expect(receiverRpc).toHaveAttribute("data-handler-payload", "0,255,128,65");

    await caller.page.locator("#abandon-rpc-invocation").dispatchEvent("click");
    await expect(receiverRpc).toHaveAttribute("data-abandoned-count", "1");

    await caller.page.locator("#invoke-failing-rpc").dispatchEvent("click");
    await expect(callerRpc).toHaveAttribute("data-error-kind", "applicationDefined");
    await expect(callerRpc).toHaveAttribute("data-error-code", "2001");
    await expect(callerRpc).toHaveAttribute("data-error-message", "remote application failure");
    await expect(callerRpc).toHaveAttribute("data-error-data", "failure-details");

    await receiver.page.locator("#unregister-echo-rpc").dispatchEvent("click");
    await expect(receiverRpc).toHaveAttribute("data-echo-registered", "false");
    await caller.page.locator("#invoke-text-rpc").dispatchEvent("click");
    await expect(callerRpc).toHaveAttribute("data-error-kind", "unsupportedMethod");
    await expect(callerRpc).toHaveAttribute("data-error-code", "1400");
    await caller.page.waitForTimeout(250);
    expect(await caller.page.locator("#room-events").getAttribute("data-count")).toBe(
      callerEventCount,
    );
    expect(await receiver.page.locator("#room-events").getAttribute("data-count")).toBe(
      receiverEventCount,
    );
  } finally {
    await Promise.all([caller.context.close(), receiver.context.close()]);
  }
});

test("Participant RPC timeout and Remote Participant leave have distinct cleanup", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(45_000);
  const room = `rpc-lifecycle-${browserName}-${Date.now()}`;
  const caller = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "rpc-waiting-caller",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "rpc-leaving-receiver",
    prepare: instrumentRpcRegistration,
  });

  try {
    const callerRpc = caller.page.locator("#participant-rpc");
    const receiverRpc = receiver.page.locator("#participant-rpc");
    await expect(caller.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    expect(await receiver.page.evaluate(() => globalThis.__rpcRegistrations)).toBe(4);

    await caller.page.locator("#invoke-timeout-rpc").dispatchEvent("click");
    await expect(receiverRpc).toHaveAttribute("data-delay-started", "true");
    await caller.page.locator("#invoke-timeout-rpc").dispatchEvent("click");
    await expect(receiverRpc).toHaveAttribute("data-handler-count", "1");
    await expect(callerRpc).toHaveAttribute("data-error-kind", "applicationError");
    await expect(callerRpc).toHaveAttribute("data-error-code", "1500");
    await expect(callerRpc).toHaveAttribute(
      "data-error-data",
      "Participant RPC handler capacity exhausted",
    );
    await expect(callerRpc).toHaveAttribute("data-error-kind", "responseTimeout", {
      timeout: 12_000,
    });
    await expect(callerRpc).toHaveAttribute("data-error-code", "1502");

    await expect(receiverRpc).toHaveAttribute("data-delay-completed", "true", {
      timeout: 5_000,
    });
    await caller.page.locator("#invoke-timeout-rpc").dispatchEvent("click");
    await expect(receiverRpc).toHaveAttribute("data-delay-completed", "false");
    await receiver.page.getByRole("button", { name: "Leave room" }).click();

    await expect(callerRpc).toHaveAttribute("data-error-kind", "remoteParticipantDisconnected");
    await expect(callerRpc).toHaveAttribute("data-error-code", "1503");
    await expect(receiverRpc).toHaveAttribute("data-delay-cancelled", "true");
    expect(await receiver.page.evaluate(() => globalThis.__rpcUnregistrations)).toBe(4);
  } finally {
    await Promise.all([caller.context.close(), receiver.context.close()]);
  }
});

test("Participant RPC caller leave resolves outstanding work as terminal", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `rpc-caller-leave-${browserName}-${Date.now()}`;
  const caller = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "rpc-leaving-caller",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "rpc-waiting-receiver",
  });

  try {
    const callerRpc = caller.page.locator("#participant-rpc");
    const receiverRpc = receiver.page.locator("#participant-rpc");
    await expect(caller.page.locator("#participant-count")).toHaveText("2");
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");

    await caller.page.locator("#invoke-timeout-rpc").dispatchEvent("click");
    await expect(receiverRpc).toHaveAttribute("data-delay-started", "true");
    await caller.page.getByRole("button", { name: "Leave room" }).click();

    await expect(callerRpc).toHaveAttribute("data-error-kind", "terminalRoomSession");
  } finally {
    await Promise.all([caller.context.close(), receiver.context.close()]);
  }
});
