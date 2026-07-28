import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant";

async function instrumentDioxusRoom(page) {
  await page.evaluate(() => {
    window.__dioxusStartAudioCalls = 0;
    const OriginalRoom = window.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args) {
        const room = Reflect.construct(target, args);
        window.__dioxusRoom = room;
        const originalStartAudio = room.startAudio;
        room.startAudio = function (...startAudioArgs) {
          window.__dioxusStartAudioCalls += 1;
          return originalStartAudio.apply(this, startAudioArgs);
        };
        return room;
      },
    });
    window.LivekitClient = new Proxy(window.LivekitClient, {
      get(target, property, receiver) {
        return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
      },
    });
  });
}

async function instrumentDioxusAttachments(page) {
  await page.evaluate(() => {
    const calls = {
      localVideo: 0,
      localAudio: 0,
      remoteVideo: 0,
      remoteAudio: 0,
      detachedElements: [],
    };
    window.__dioxusAttachmentCalls = calls;
    for (const [constructorName, className, counter] of [
      ["LocalVideoTrack", "dioxus-local-video", "localVideo"],
      ["LocalAudioTrack", "dioxus-local-audio", "localAudio"],
      ["RemoteVideoTrack", "dioxus-remote-video", "remoteVideo"],
      ["RemoteAudioTrack", "dioxus-remote-audio", "remoteAudio"],
    ]) {
      const prototype = window.LivekitClient[constructorName].prototype;
      const original = prototype.detach;
      prototype.detach = function (element) {
        if (element?.classList?.contains(className)) {
          calls[counter] += 1;
          calls.detachedElements.push(element);
        }
        return original.call(this, element);
      };
    }
  });
}

async function instrumentDioxusRoomAndAttachments(page) {
  await instrumentDioxusRoom(page);
  await instrumentDioxusAttachments(page);
}

test("provider hooks react while media nodes only control attachment", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const room = `dioxus-integration-${browserName}-${Date.now()}`;
  const teacher = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "teacher",
    prepare: instrumentDioxusRoomAndAttachments,
  });
  const student = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "student",
    prepare: instrumentDioxusAttachments,
  });

  try {
    const teacherProvider = teacher.page.locator("#dioxus-room-session");
    const studentProvider = student.page.locator("#dioxus-room-session");
    await expect(teacherProvider).toHaveAttribute("data-connected", "true");
    await expect(teacherProvider).toHaveAttribute("data-remote-participant-count", "1");
    await expect(teacherProvider).toHaveAttribute("data-webrtc", "true");
    await expect(teacherProvider).toHaveAttribute("data-playback", /^(allowed|blocked)$/);

    await teacher.page.locator("#dioxus-start-audio-playback").dispatchEvent("click");
    await expect(teacherProvider).toHaveAttribute("data-playback-action", "ready");
    expect(await teacher.page.evaluate(() => window.__dioxusStartAudioCalls)).toBe(1);
    await teacher.page.locator("#dioxus-allow-all-subscriptions").dispatchEvent("click");
    await expect(teacherProvider).toHaveAttribute("data-permissions-action", "ready");
    await teacher.page.locator("#dioxus-set-metadata").dispatchEvent("click");
    await expect(teacherProvider).toHaveAttribute("data-metadata-action", "ready");
    await expect(teacher.page.locator("#room-session")).toHaveAttribute(
      "data-metadata",
      "dioxus-action-metadata",
    );
    await teacher.page.locator("#dioxus-update-attributes").dispatchEvent("click");
    await expect(teacherProvider).toHaveAttribute("data-attributes-action", "ready");
    await expect(
      teacher.page.locator(
        '.local-participant-attribute[data-key="dioxus-action"][data-value="complete"]',
      ),
    ).toHaveCount(1);

    await student.page.getByRole("button", { name: "Start camera" }).click();
    await expect(student.page.locator("#camera-state")).toHaveText("on");
    await student.page.getByRole("button", { name: "Start microphone" }).click();
    await expect(student.page.locator("#microphone-state")).toHaveText("on");
    await student.page.getByRole("button", { name: "Publish camera" }).click();
    await expect(student.page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(
      1,
    );
    await student.page.getByRole("button", { name: "Publish microphone" }).click();
    await expect(student.page.locator('.local-track-publication[data-kind="audio"]')).toHaveCount(
      1,
    );

    await expect(studentProvider).toHaveAttribute("data-local-publication-count", "2");
    await expect(teacherProvider).toHaveAttribute("data-remote-publication-count", "2");
    const teacherDetachBefore = await teacher.page.evaluate(() => ({
      remoteVideo: window.__dioxusAttachmentCalls.remoteVideo,
      remoteAudio: window.__dioxusAttachmentCalls.remoteAudio,
    }));
    const studentDetachBefore = await student.page.evaluate(() => ({
      localVideo: window.__dioxusAttachmentCalls.localVideo,
      localAudio: window.__dioxusAttachmentCalls.localAudio,
    }));

    await teacher.page.locator("#toggle-dioxus-media-nodes").dispatchEvent("click");
    await student.page.locator("#toggle-dioxus-media-nodes").dispatchEvent("click");
    await expect(teacher.page.locator(".dioxus-remote-video")).toHaveCount(1);
    await expect(teacher.page.locator(".dioxus-remote-audio")).toHaveCount(1);
    await expect(student.page.locator(".dioxus-local-video")).toHaveCount(1);
    await expect(student.page.locator(".dioxus-local-audio")).toHaveCount(1);
    await expect
      .poll(() =>
        teacher.page
          .locator(".dioxus-remote-video")
          .evaluate((element) => element.srcObject?.active === true),
      )
      .toBe(true);

    await teacher.page.locator(".dioxus-remote-video").evaluate((element) => {
      window.__dioxusRemoteVideoElement = element;
    });
    await teacher.page.evaluate(() => {
      const room = window.__dioxusRoom;
      const participant = room.remoteParticipants.get("student");
      const publication = [...participant.videoTrackPublications.values()][0];
      const oldTrack = publication.track;
      window.__dioxusOldTrackDetachCount = 0;
      const originalDetach = oldTrack.detach.bind(oldTrack);
      oldTrack.detach = (...args) => {
        window.__dioxusOldTrackDetachCount += 1;
        return originalDetach(...args);
      };
      const forward = (target, overrides = {}) => ({
        get(_target, property) {
          if (Object.prototype.hasOwnProperty.call(overrides, property)) {
            return overrides[property];
          }
          const value = Reflect.get(target, property, target);
          return typeof value === "function" ? value.bind(target) : value;
        },
        set(_target, property, value) {
          return Reflect.set(target, property, value, target);
        },
      });
      const replacementTrack = new Proxy(oldTrack, forward(oldTrack));
      const replacementPublication = new Proxy(
        publication,
        forward(publication, { track: replacementTrack }),
      );
      const replacementPublications = new Map(participant.trackPublications);
      replacementPublications.set(publication.trackSid, replacementPublication);
      const replacementParticipant = new Proxy(
        participant,
        forward(participant, { trackPublications: replacementPublications }),
      );
      room.remoteParticipants.set("student", replacementParticipant);
      room.emit(
        window.LivekitClient.RoomEvent.TrackSubscribed,
        replacementTrack,
        replacementPublication,
        replacementParticipant,
      );
    });
    await expect
      .poll(() => teacher.page.evaluate(() => window.__dioxusOldTrackDetachCount))
      .toBeGreaterThan(0);
    await expect
      .poll(() =>
        teacher.page.locator(".dioxus-remote-video").evaluate(
          (element) =>
            element === window.__dioxusRemoteVideoElement && element.srcObject?.active === true,
        ),
      )
      .toBe(true);
    await expect
      .poll(() =>
        student.page
          .locator(".dioxus-local-video")
          .evaluate((element) => element.srcObject?.active === true),
      )
      .toBe(true);

    await teacher.page.locator("#toggle-dioxus-media-nodes").dispatchEvent("click");
    await expect(teacher.page.locator(".dioxus-remote-video")).toHaveCount(0);
    await expect(teacher.page.locator(".dioxus-remote-audio")).toHaveCount(0);
    await expect(
      teacher.page.locator(
        '.remote-participant[data-identity="student"] .remote-track-publication[data-kind="video"]',
      ),
    ).toHaveAttribute("data-subscribed", "true");
    await expect(
      teacher.page.locator(
        '.remote-participant[data-identity="student"] .remote-track-publication[data-kind="audio"]',
      ),
    ).toHaveAttribute("data-subscribed", "true");

    await student.page.locator("#toggle-dioxus-media-nodes").dispatchEvent("click");
    await expect(student.page.locator(".dioxus-local-video")).toHaveCount(0);
    await expect(student.page.locator(".dioxus-local-audio")).toHaveCount(0);
    await expect(student.page.locator("#local-video-track")).toHaveAttribute(
      "data-stopped",
      "false",
    );
    await expect(student.page.locator("#local-audio-track")).toHaveAttribute(
      "data-stopped",
      "false",
    );
    await expect(studentProvider).toHaveAttribute("data-local-publication-count", "2");
    await expect
      .poll(() =>
        teacher.page.evaluate((before) => ({
          remoteVideo:
            window.__dioxusAttachmentCalls.remoteVideo > before.remoteVideo,
          remoteAudio:
            window.__dioxusAttachmentCalls.remoteAudio > before.remoteAudio,
        }), teacherDetachBefore),
      )
      .toEqual({
        remoteVideo: true,
        remoteAudio: true,
      });
    await expect
      .poll(() =>
        student.page.evaluate((before) => ({
          localVideo:
            window.__dioxusAttachmentCalls.localVideo > before.localVideo,
          localAudio:
            window.__dioxusAttachmentCalls.localAudio > before.localAudio,
        }), studentDetachBefore),
      )
      .toEqual({
        localVideo: true,
        localAudio: true,
      });
    expect(
      await teacher.page.evaluate(() =>
        window.__dioxusAttachmentCalls.detachedElements.every(
          (element) => element.srcObject == null,
        ),
      ),
    ).toBe(true);
    expect(
      await student.page.evaluate(() =>
        window.__dioxusAttachmentCalls.detachedElements.every(
          (element) => element.srcObject == null,
        ),
      ),
    ).toBe(true);

    await teacher.page.locator("#toggle-dioxus-media-nodes").dispatchEvent("click");
    await student.page.locator("#toggle-dioxus-media-nodes").dispatchEvent("click");
    await expect(teacher.page.locator(".dioxus-remote-video")).toHaveCount(1);
    await expect(student.page.locator(".dioxus-local-video")).toHaveCount(1);
    await expect
      .poll(() =>
        teacher.page
          .locator(".dioxus-remote-video")
          .evaluate((element) => element.srcObject?.active === true),
      )
      .toBe(true);

    const remoteVideoPublication = teacher.page.locator(
      '.remote-participant[data-identity="student"] .remote-track-publication[data-kind="video"]',
    );
    await teacher.page
      .locator('.dioxus-toggle-remote-subscription[data-kind="video"]')
      .dispatchEvent("click");
    await expect(teacherProvider).toHaveAttribute("data-remote-subscription-action", "ready");
    await expect(remoteVideoPublication).toHaveAttribute("data-subscribed", "false");
  } finally {
    await Promise.all([teacher.context.close(), student.context.close()]);
  }
});

test("actions and provider cleanup preserve local capture at a mobile viewport", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `dioxus-cleanup-${browserName}-${Date.now()}`;
  const participant = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "mobile-participant",
    prepare: (page) =>
      page.evaluate(() => {
        window.__dioxusDisconnects = 0;
        const OriginalRoom = window.LivekitClient.Room;
        const InstrumentedRoom = new Proxy(OriginalRoom, {
          construct(target, args) {
            const room = Reflect.construct(target, args);
            const originalDisconnect = room.disconnect;
            room.disconnect = function (...disconnectArgs) {
              window.__dioxusDisconnects += 1;
              return originalDisconnect.apply(this, disconnectArgs);
            };
            return room;
          },
        });
        window.LivekitClient = new Proxy(window.LivekitClient, {
          get(target, property, receiver) {
            return property === "Room"
              ? InstrumentedRoom
              : Reflect.get(target, property, receiver);
          },
        });
      }),
  });

  try {
    await participant.page.setViewportSize({ width: 390, height: 844 });
    await participant.page.getByRole("button", { name: "Start camera" }).click();
    await expect(participant.page.locator("#local-video-track")).toHaveAttribute(
      "data-stopped",
      "false",
    );
    await participant.page.locator("#change-dioxus-event-capacity").dispatchEvent("click");
    await expect(participant.page.locator("#connection-state")).toHaveText("connected");
    expect(await participant.page.evaluate(() => window.__dioxusDisconnects)).toBe(0);

    await participant.page.locator("#dioxus-leave-room").dispatchEvent("click");
    await expect(participant.page.locator("#connection-state")).toHaveText("disconnected");
    await expect(participant.page.locator("#dioxus-room-session")).toHaveAttribute(
      "data-connected",
      "false",
    );
    await expect
      .poll(() => participant.page.evaluate(() => window.__dioxusDisconnects))
      .toBe(1);
    await expect(participant.page.locator("#local-video-track")).toHaveAttribute(
      "data-stopped",
      "false",
    );

    await participant.page.getByRole("button", { name: "Join room" }).click();
    await expect(participant.page.locator("#connection-state")).toHaveText("connected", {
      timeout: 30_000,
    });
    await expect(participant.page.locator("#dioxus-room-session")).toHaveAttribute(
      "data-connected",
      "true",
    );

    await participant.page.locator("#drop-room-session").dispatchEvent("click");
    await expect(participant.page.locator("#dioxus-room-session")).toHaveCount(0);
    await expect
      .poll(() => participant.page.evaluate(() => window.__dioxusDisconnects))
      .toBe(2);
    await expect(participant.page.locator("#local-video-track")).toHaveAttribute(
      "data-stopped",
      "false",
    );
    expect(
      await participant.page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    ).toBe(true);
  } finally {
    await participant.context.close();
  }
});

test("provider unmount leaves an externally retained Room Session active", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const participant = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room: `dioxus-retained-${browserName}-${Date.now()}`,
    identity: "retained-owner",
    prepare: (page) =>
      page.evaluate(() => {
        window.__dioxusDisconnects = 0;
        const OriginalRoom = window.LivekitClient.Room;
        const InstrumentedRoom = new Proxy(OriginalRoom, {
          construct(target, args) {
            const room = Reflect.construct(target, args);
            const originalDisconnect = room.disconnect;
            room.disconnect = function (...disconnectArgs) {
              window.__dioxusDisconnects += 1;
              return originalDisconnect.apply(this, disconnectArgs);
            };
            return room;
          },
        });
        window.LivekitClient = new Proxy(window.LivekitClient, {
          get(target, property, receiver) {
            return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
          },
        });
      }),
  });

  try {
    await participant.page
      .locator("#retain-and-unmount-room-provider")
      .dispatchEvent("click");
    await expect(participant.page.locator("#dioxus-room-session")).toHaveCount(0);
    const retained = participant.page.locator("#retained-room-session");
    await expect(retained).toHaveAttribute("data-present", "true");
    await expect(retained).toHaveAttribute("data-terminal", "false");
    await expect(retained).toHaveAttribute("data-connection", "connected");
    expect(await participant.page.evaluate(() => window.__dioxusDisconnects)).toBe(0);

    await participant.page.locator("#release-retained-room-session").dispatchEvent("click");
    await expect(retained).toHaveAttribute("data-present", "false");
    await expect
      .poll(() => participant.page.evaluate(() => window.__dioxusDisconnects))
      .toBe(1);
  } finally {
    await participant.context.close();
  }
});
