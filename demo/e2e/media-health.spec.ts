import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant";

async function openParticipant(browser, browserName, baseURL, room, identity) {
  return openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity,
    prepare: (page) =>
      page.evaluate(() => {
        const OriginalRoom = globalThis.LivekitClient.Room;
        const InstrumentedRoom = new Proxy(OriginalRoom, {
          construct(target, args) {
            const room = Reflect.construct(target, args);
            globalThis.__mediaHealthRoom = room;
            return room;
          },
        });
        globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
          get(target, property, receiver) {
            return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
          },
        });
      }),
  });
}

async function publishCameraAndMicrophone(page) {
  await page.getByRole("button", { name: "Start camera" }).click();
  await expect(page.locator("#camera-state")).toHaveText("on");
  await page.getByRole("button", { name: "Start microphone" }).click();
  await expect(page.locator("#microphone-state")).toHaveText("on");
  await page.getByRole("button", { name: "Publish camera" }).click();
  await expect(page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(1);
  await page.getByRole("button", { name: "Publish microphone" }).click();
  await expect(page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(1);
}

const remoteMedia = (page, identity, kind) =>
  page.locator(
    `.remote-media-publication[data-participant-identity="${identity}"][data-kind="${kind}"]`,
  );

test("participant quality and on-demand media statistics stay typed and coherent", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const room = `media-health-${browserName}-${Date.now()}`;
  const teacher = await openParticipant(browser, browserName, baseURL, room, "teacher");
  const student = await openParticipant(browser, browserName, baseURL, room, "student");

  try {
    await student.page.locator("#observe-room-events").dispatchEvent("click");
    await expect(student.page.locator("#room-events")).toHaveAttribute("data-observing", "true");
    await publishCameraAndMicrophone(teacher.page);

    const teacherParticipant = student.page.locator(
      '.remote-participant[data-identity="teacher"]',
    );
    await expect(teacherParticipant).toHaveAttribute(
      "data-connection-quality",
      /^(excellent|good|poor|lost|unknown)$/,
    );
    await expect(teacher.page.locator("#room-session")).toHaveAttribute(
      "data-connection-quality",
      /^(excellent|good|poor|lost|unknown)$/,
    );

    await student.page.evaluate(() => {
      const participant = globalThis.__mediaHealthRoom.remoteParticipants.get("teacher");
      Object.defineProperty(participant, "connectionQuality", {
        configurable: true,
        writable: true,
        value: "poor",
      });
      globalThis.__mediaHealthRoom.emit(
        globalThis.LivekitClient.RoomEvent.ConnectionQualityChanged,
        "poor",
        participant,
      );
      participant.connectionQuality = "excellent";
      globalThis.__mediaHealthRoom.emit(
        globalThis.LivekitClient.RoomEvent.ConnectionQualityChanged,
        "excellent",
        participant,
      );
    });
    await expect(teacherParticipant).toHaveAttribute("data-connection-quality", "excellent");
    await expect(student.page.locator("#room-events")).toHaveAttribute(
      "data-event-type",
      "participantConnectionQualityChanged",
    );
    await expect(student.page.locator("#room-events")).toHaveAttribute(
      "data-event-identity",
      "teacher",
    );
    await expect(student.page.locator("#room-events")).toHaveAttribute(
      "data-event-quality",
      "excellent",
    );
    const eventState = await student.page.locator("#room-events").evaluate((element) => ({
      eventRevision: element.dataset.eventRevision,
      snapshotRevision: element.dataset.snapshotRevision,
    }));
    expect(eventState.eventRevision).toBe(eventState.snapshotRevision);

    const localStatistics = teacher.page.locator("#local-media-statistics");
    await expect
      .poll(async () => {
        await teacher.page.locator("#refresh-local-media-statistics").dispatchEvent("click");
        return Number(await localStatistics.getAttribute("data-audio-timestamp"));
      })
      .toBeGreaterThan(0);
    await expect
      .poll(async () => {
        await teacher.page.locator("#refresh-local-media-statistics").dispatchEvent("click");
        return Math.max(
          Number(await localStatistics.getAttribute("data-audio-packets-sent")),
          Number(await localStatistics.getAttribute("data-audio-bytes-sent")),
        );
      })
      .toBeGreaterThan(0);
    await expect
      .poll(async () => {
        await teacher.page.locator("#refresh-local-media-statistics").dispatchEvent("click");
        return Number(await localStatistics.getAttribute("data-video-layer-count"));
      })
      .toBeGreaterThan(0);
    await expect
      .poll(async () => {
        await teacher.page.locator("#refresh-local-media-statistics").dispatchEvent("click");
        return Number(await localStatistics.getAttribute("data-video-frames-sent"));
      })
      .toBeGreaterThan(0);

    const remoteAudio = remoteMedia(student.page, "teacher", "audio");
    const remoteVideo = remoteMedia(student.page, "teacher", "video");
    await expect(remoteAudio).toHaveAttribute("data-attached", "true", { timeout: 20_000 });
    await expect(remoteVideo).toHaveAttribute("data-attached", "true", { timeout: 20_000 });
    await expect
      .poll(async () => {
        await remoteAudio.locator(".refresh-remote-statistics").dispatchEvent("click");
        return Number(await remoteAudio.getAttribute("data-statistics-timestamp"));
      })
      .toBeGreaterThan(0);
    await expect
      .poll(async () => {
        await remoteAudio.locator(".refresh-remote-statistics").dispatchEvent("click");
        return Math.max(
          Number(await remoteAudio.getAttribute("data-statistics-packets-received")),
          Number(await remoteAudio.getAttribute("data-statistics-bytes-received")),
          Number(await remoteAudio.getAttribute("data-statistics-total-audio-energy")),
          Number(await remoteAudio.getAttribute("data-statistics-total-samples-duration")),
        );
      })
      .toBeGreaterThan(0);
    await expect
      .poll(async () => {
        await remoteVideo.locator(".refresh-remote-statistics").dispatchEvent("click");
        return Number(await remoteVideo.getAttribute("data-statistics-frames-received"));
      })
      .toBeGreaterThan(0);

    await student.page.evaluate(() => {
      const publication = globalThis.__mediaHealthRoom.remoteParticipants
        .get("teacher")
        .videoTrackPublications.values()
        .next().value;
      const track = publication.track;
      const original = track.getReceiverStats.bind(track);
      track.getReceiverStats = async () => {
        const statistics = await original();
        await new Promise((resolve) => (globalThis.__releaseReceiverStatistics = resolve));
        return statistics;
      };
    });
    await remoteVideo.locator(".refresh-remote-statistics").dispatchEvent("click");
    await expect
      .poll(() => student.page.evaluate(() => typeof globalThis.__releaseReceiverStatistics))
      .toBe("function");
    await remoteVideo.locator(".remote-subscription-button").dispatchEvent("click");
    await expect(remoteVideo).toHaveAttribute("data-media-available", "false");
    await student.page.evaluate(() => globalThis.__releaseReceiverStatistics());
    await expect(student.page.getByRole("alert")).toContainText(
      "could not query LiveKit media statistics",
    );

    await teacher.page.getByRole("button", { name: "Unpublish camera" }).click();
    await expect(teacher.page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(0);
    await teacher.page.evaluate(() => {
      const publication = globalThis.__mediaHealthRoom.localParticipant.audioTrackPublications
        .values()
        .next().value;
      const track = publication.track;
      const original = track.getSenderStats.bind(track);
      track.getSenderStats = async () => {
        const statistics = await original();
        await new Promise((resolve) => (globalThis.__releaseSenderStatistics = resolve));
        return statistics;
      };
    });
    await teacher.page.locator("#refresh-local-media-statistics").dispatchEvent("click");
    await expect
      .poll(() => teacher.page.evaluate(() => typeof globalThis.__releaseSenderStatistics))
      .toBe("function");
    await teacher.page.getByRole("button", { name: "Leave room" }).click();
    await teacher.page.evaluate(() => globalThis.__releaseSenderStatistics());
    await expect(teacher.page.getByRole("alert")).toContainText(
      "could not query LiveKit media statistics",
    );
  } finally {
    await Promise.all([teacher.context.close(), student.context.close()]);
  }
});
