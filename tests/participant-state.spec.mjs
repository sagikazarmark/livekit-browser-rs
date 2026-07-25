import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant.mjs";

async function exposeRoom(page) {
  await page.evaluate(() => {
    const OriginalRoom = globalThis.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args) {
        const room = Reflect.construct(target, args);
        globalThis.__livekitTestRoom = room;
        return room;
      },
    });
    globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
      get(target, property, receiver) {
        return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
      },
    });
  });
}

test("participant application state propagates through coherent snapshots", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `participant-state-${browserName}-${Date.now()}`;
  const editor = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "state-editor",
    prepare: exposeRoom,
  });
  const observer = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "state-observer",
    prepare: exposeRoom,
  });

  try {
    await editor.page.locator("#observe-room-events").dispatchEvent("click");
    await observer.page.locator("#observe-room-events").dispatchEvent("click");
    const local = editor.page.locator("#room-session");
    const remote = observer.page.locator('.remote-participant[data-identity="state-editor"]');
    await expect(local).toHaveAttribute("data-can-update-metadata", "true");
    await expect(local).toHaveAttribute("data-room-metadata", "");
    await expect(remote).toHaveCount(1);
    await expect(local).toHaveAttribute("data-remote-participant-count", "1");

    await editor.page.evaluate(() => {
      const room = globalThis.__livekitTestRoom;
      const observer = room.remoteParticipants.get("state-observer");
      Object.defineProperty(room, "activeSpeakers", {
        configurable: true,
        value: [observer, room.localParticipant, observer],
      });
      room.emit(globalThis.LivekitClient.RoomEvent.ActiveSpeakersChanged, room.activeSpeakers);
    });
    await expect(local).toHaveAttribute(
      "data-active-speakers",
      "state-observer|state-editor",
    );
    await expect(editor.page.locator("#room-events")).toHaveAttribute(
      "data-event-type",
      "activeSpeakersChanged",
    );
    await expect(editor.page.locator("#room-events")).toHaveAttribute(
      "data-event-revision",
      await local.getAttribute("data-revision"),
    );

    const localRevision = Number(await local.getAttribute("data-revision"));
    const remoteRevision = Number(
      await observer.page.locator("#room-session").getAttribute("data-revision"),
    );
    await editor.page.getByLabel("Participant metadata").fill("ready");
    await editor.page.getByRole("button", { name: "Update participant metadata" }).click();

    await expect(local).toHaveAttribute("data-metadata", "ready");
    await expect(remote).toHaveAttribute("data-metadata", "ready");
    const nextLocalRevision = Number(await local.getAttribute("data-revision"));
    const nextRemoteRevision = Number(
      await observer.page.locator("#room-session").getAttribute("data-revision"),
    );
    expect(nextLocalRevision).toBeGreaterThan(localRevision);
    expect(nextRemoteRevision).toBeGreaterThan(remoteRevision);
    await expect(editor.page.locator("#room-events")).toHaveAttribute(
      "data-event-revision",
      String(nextLocalRevision),
    );
    await expect(observer.page.locator("#room-events")).toHaveAttribute(
      "data-event-revision",
      String(nextRemoteRevision),
    );

    await editor.page.getByLabel("Participant attribute key").fill("status");
    await editor.page.getByLabel("Participant attribute value").fill("present");
    await editor.page.getByRole("button", { name: "Update participant attribute" }).click();

    const localStatus = editor.page.locator(
      '.local-participant-attribute[data-key="status"]',
    );
    const remoteStatus = remote.locator('.participant-attribute[data-key="status"]');
    await expect(localStatus).toHaveAttribute("data-value", "present");
    await expect(remoteStatus).toHaveAttribute("data-value", "present");

    await editor.page.getByLabel("Participant attribute key").fill("topic");
    await editor.page.getByLabel("Participant attribute value").fill("waves");
    await editor.page.getByRole("button", { name: "Update participant attribute" }).click();
    await expect(
      editor.page.locator('.local-participant-attribute[data-key="topic"]'),
    ).toHaveAttribute("data-value", "waves");
    await expect(localStatus).toHaveAttribute("data-value", "present");

    await editor.page.getByLabel("Participant attribute key").fill("status");
    await editor.page.getByRole("button", { name: "Remove participant attribute" }).click();
    await expect(localStatus).toHaveCount(0);
    await expect(remoteStatus).toHaveCount(0);
    await expect(
      editor.page.locator('.local-participant-attribute[data-key="topic"]'),
    ).toHaveAttribute("data-value", "waves");

    const roomRevision = Number(await local.getAttribute("data-revision"));
    await editor.page.evaluate(() => {
      const room = globalThis.__livekitTestRoom;
      Object.defineProperty(room, "metadata", {
        configurable: true,
        value: "chapter-2",
      });
      room.emit(globalThis.LivekitClient.RoomEvent.RoomMetadataChanged, room.metadata);
    });
    await expect(local).toHaveAttribute("data-room-metadata", "chapter-2");
    const nextRoomRevision = Number(await local.getAttribute("data-revision"));
    expect(nextRoomRevision).toBe(roomRevision + 1);
    await expect(editor.page.locator("#room-events")).toHaveAttribute(
      "data-event-revision",
      String(nextRoomRevision),
    );
    await expect(editor.page.locator("#room-events")).toHaveAttribute(
      "data-snapshot-revision",
      String(nextRoomRevision),
    );
  } finally {
    await Promise.all([editor.context.close(), observer.context.close()]);
  }
});

test("concurrent participant updates preserve metadata and attributes", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const participant = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room: `participant-state-ordering-${browserName}-${Date.now()}`,
    identity: "state-editor",
  });

  try {
    const session = participant.page.locator("#room-session");
    await participant.page.getByLabel("Participant metadata").fill("in-session");
    await participant.page.getByLabel("Participant attribute key").fill("mode");
    await participant.page.getByLabel("Participant attribute value").fill("collaborative");
    await participant.page.evaluate(() => {
      document.querySelector("#update-participant-metadata").click();
      document.querySelector("#update-participant-attribute").click();
    });

    await expect(session).toHaveAttribute("data-metadata", "in-session");
    await expect(
      participant.page.locator('.local-participant-attribute[data-key="mode"]'),
    ).toHaveAttribute("data-value", "collaborative");

    await participant.page.getByLabel("Participant metadata").fill("");
    await participant.page.getByRole("button", { name: "Update participant metadata" }).click();
    await expect(participant.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "participantUpdateFailed",
    );
    await expect(session).toHaveAttribute("data-metadata", "in-session");
  } finally {
    await participant.context.close();
  }
});

test("dropped participant update futures leave eventual state observable", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const participant = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room: `participant-state-abandoned-${browserName}-${Date.now()}`,
    identity: "state-editor",
  });

  try {
    const session = participant.page.locator("#room-session");
    await participant.page.getByLabel("Participant metadata").fill("eventual");
    await participant.page.getByLabel("Participant attribute key").fill("phase");
    await participant.page.getByLabel("Participant attribute value").fill("complete");

    await participant.page.locator("#abandon-participant-updates").dispatchEvent("click");

    await expect(session).toHaveAttribute("data-metadata", "eventual");
    await expect(
      participant.page.locator('.local-participant-attribute[data-key="phase"]'),
    ).toHaveAttribute("data-value", "complete");
  } finally {
    await participant.context.close();
  }
});

test("denied participant updates remain typed and non-optimistic", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const participant = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room: `participant-state-denied-${browserName}-${Date.now()}`,
    identity: "participant-state-denied",
  });

  try {
    const session = participant.page.locator("#room-session");
    await expect(session).toHaveAttribute("data-can-update-metadata", "false");

    await participant.page.getByLabel("Participant metadata").fill("not-authoritative");
    await participant.page.getByRole("button", { name: "Update participant metadata" }).click();
    await expect(participant.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    await expect(session).toHaveAttribute("data-metadata", "");

    await participant.page.getByLabel("Participant metadata").fill("");
    await participant.page.getByRole("button", { name: "Update participant metadata" }).click();
    await expect(participant.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    await expect(session).toHaveAttribute("data-metadata", "");

    await participant.page.getByLabel("Participant attribute key").fill("status");
    await participant.page.getByLabel("Participant attribute value").fill("not-authoritative");
    await participant.page.getByRole("button", { name: "Update participant attribute" }).click();
    await expect(participant.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    await expect(session).toHaveAttribute("data-metadata", "");
    await expect(participant.page.locator(".local-participant-attribute")).toHaveCount(0);
  } finally {
    await participant.context.close();
  }
});
