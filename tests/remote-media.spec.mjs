import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant.mjs";

async function openParticipant(
  browser,
  browserName,
  baseURL,
  room,
  identity,
  forceAudioBlocked = false,
) {
  return openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity,
    prepare: (page) =>
      page.evaluate((blocked) => {
        if (blocked) {
          const originalPlay = HTMLMediaElement.prototype.play;
          globalThis.__remoteMediaAudioAllowed = false;
          document.addEventListener(
            "click",
            (event) => {
              if (event.target instanceof Element && event.target.closest(".audio-button")) {
                globalThis.__remoteMediaAudioActivation = navigator.userActivation.isActive;
                globalThis.__remoteMediaAudioAllowed = true;
              }
            },
            true,
          );
          HTMLMediaElement.prototype.play = function (...args) {
            if (this instanceof HTMLAudioElement && !globalThis.__remoteMediaAudioAllowed) {
              return Promise.reject(
                new DOMException("synthetic autoplay policy block", "NotAllowedError"),
              );
            }
            return originalPlay.apply(this, args);
          };
        }
        const OriginalRoom = globalThis.LivekitClient.Room;
        const InstrumentedRoom = new Proxy(OriginalRoom, {
          construct(target, args) {
            const room = Reflect.construct(target, args);
            globalThis.__remoteMediaRoom = room;
            const originalConnect = room.connect.bind(room);
            room.connect = (url, token, options) => {
              globalThis.__remoteMediaConnectOptions = structuredClone(options);
              return originalConnect(url, token, options);
            };
            return room;
          },
        });
        globalThis.LivekitClient = new Proxy(globalThis.LivekitClient, {
          get(target, property, receiver) {
            return property === "Room" ? InstrumentedRoom : Reflect.get(target, property, receiver);
          },
        });
      }, forceAudioBlocked),
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

function remoteMedia(page, identity, kind) {
  return page.locator(
    `.remote-media-publication[data-participant-identity="${identity}"][data-kind="${kind}"]`,
  );
}

test("two participants exchange typed Remote Media Tracks with independent lifecycles", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const room = `remote-media-${browserName}-${Date.now()}`;
  const teacher = await openParticipant(browser, browserName, baseURL, room, "teacher");
  const student = await openParticipant(browser, browserName, baseURL, room, "student");

  try {
    await publishCameraAndMicrophone(teacher.page);
    await publishCameraAndMicrophone(student.page);

    for (const [participant, remoteIdentity] of [
      [teacher, "student"],
      [student, "teacher"],
    ]) {
      expect(await participant.page.evaluate(() => window.__remoteMediaConnectOptions)).toEqual({
        autoSubscribe: true,
      });
      for (const kind of ["video", "audio"]) {
        const media = remoteMedia(participant.page, remoteIdentity, kind);
        await expect(media).toHaveAttribute("data-subscribed", "true", { timeout: 20_000 });
        await expect(media).toHaveAttribute("data-media-available", "true");
        await expect(media).toHaveAttribute("data-attached", "true");
        await expect(media.locator(".remote-subscription-button")).toBeVisible();
        await expect
          .poll(() =>
            media
              .locator(kind)
              .evaluate((element) => element.srcObject?.active === true),
          )
          .toBe(true);
      }
    }

    const teacherVideo = remoteMedia(teacher.page, "student", "video");
    await teacherVideo.locator(".attach-sibling-remote-video").dispatchEvent("click");
    await expect(teacherVideo).toHaveAttribute("data-sibling-attached", "true");
    await teacherVideo.locator(".close-remote-video-attachment").dispatchEvent("click");
    await expect(teacherVideo).toHaveAttribute("data-attached", "false");
    await expect(teacherVideo).toHaveAttribute("data-sibling-attached", "true");
    await expect(
      teacher.page.locator(
        '.remote-participant[data-identity="student"] .remote-track-publication[data-kind="video"]',
      ),
    ).toHaveAttribute("data-subscribed", "true");
    await expect
      .poll(() =>
        teacherVideo.locator("video").evaluate((element) => element.srcObject?.active === true),
      )
      .toBe(true);
    await teacherVideo
      .locator(".drop-sibling-remote-video-attachment")
      .dispatchEvent("click");
    await expect(teacherVideo).toHaveAttribute("data-sibling-attached", "false");
    await expect
      .poll(() => teacherVideo.locator("video").evaluate((element) => element.srcObject))
      .toBeNull();

    const teacherAudio = remoteMedia(teacher.page, "student", "audio");
    await teacherAudio.locator("audio").evaluate((element) => {
      window.__remoteMediaDroppedAudioElement = element;
    });
    await teacherAudio.locator(".drop-remote-audio-attachment").dispatchEvent("click");
    await expect(teacherAudio).toHaveAttribute("data-attached", "false");
    await expect(
      teacher.page.locator(
        '.remote-participant[data-identity="student"] .remote-track-publication[data-kind="audio"]',
      ),
    ).toHaveAttribute("data-subscribed", "true");
    await expect
      .poll(() => teacher.page.evaluate(() => window.__remoteMediaDroppedAudioElement.srcObject))
      .toBeNull();

    const studentAudio = remoteMedia(student.page, "teacher", "audio");
    await studentAudio.locator("audio").evaluate((element) => {
      window.__remoteMediaOldAudioElement = element;
    });
    await studentAudio.locator(".remote-subscription-button").dispatchEvent("click");
    await expect(studentAudio).toHaveAttribute("data-subscribed", "false");
    await expect(studentAudio).toHaveAttribute("data-media-available", "false");
    await expect(
      student.page.locator(
        '.remote-participant[data-identity="teacher"] .remote-track-publication[data-kind="audio"]',
      ),
    ).toHaveCount(1);
    await expect
      .poll(() => student.page.evaluate(() => window.__remoteMediaOldAudioElement.srcObject))
      .toBeNull();

    await studentAudio.locator(".remote-subscription-button").dispatchEvent("click");
    await expect(studentAudio).toHaveAttribute("data-subscribed", "true");
    await expect(studentAudio).toHaveAttribute("data-media-available", "true");
    await expect(studentAudio).toHaveAttribute("data-attached", "true");
    await expect
      .poll(() =>
        studentAudio.locator("audio").evaluate((element) => element.srcObject?.active === true),
      )
      .toBe(true);

    const studentVideo = remoteMedia(student.page, "teacher", "video");
    await studentVideo.locator("video").evaluate((element) => {
      window.__remoteMediaEndedVideoElement = element;
    });
    await student.page.evaluate(() => {
      const publication = window.__remoteMediaRoom.remoteParticipants
        .get("teacher")
        .videoTrackPublications.values()
        .next().value;
      const track = publication.track;
      track.emit(window.LivekitClient.TrackEvent.Ended, track);
    });
    await expect(studentVideo).toHaveAttribute("data-media-available", "false");
    await expect(
      student.page.locator(
        '.remote-participant[data-identity="teacher"] .remote-track-publication[data-kind="video"]',
      ),
    ).toHaveCount(1);
    await expect
      .poll(() => student.page.evaluate(() => window.__remoteMediaEndedVideoElement.srcObject))
      .toBeNull();
  } finally {
    await Promise.all([teacher.context.close(), student.context.close()]);
  }
});

test("blocked remote audio is observable and retried during user activation", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `remote-audio-playback-${browserName}-${Date.now()}`;
  const listener = await openParticipant(
    browser,
    browserName,
    baseURL,
    room,
    "listener",
    true,
  );
  const speaker = await openParticipant(browser, browserName, baseURL, room, "speaker");

  try {
    await speaker.page.getByRole("button", { name: "Start microphone" }).click();
    await expect(speaker.page.locator("#microphone-state")).toHaveText("on");
    await speaker.page.getByRole("button", { name: "Publish microphone" }).click();
    const audio = remoteMedia(listener.page, "speaker", "audio");
    await expect(audio).toHaveAttribute("data-attached", "true", { timeout: 30_000 });
    await expect(listener.page.locator("#audio-playback")).toHaveAttribute(
      "data-state",
      "blocked",
    );
    await listener.page.getByRole("button", { name: "Enable remote audio" }).click();
    await expect(listener.page.locator("#audio-playback")).toHaveAttribute(
      "data-state",
      "allowed",
    );
    expect(await listener.page.evaluate(() => window.__remoteMediaAudioActivation)).toBe(true);
    await expect
      .poll(() => audio.locator("audio").evaluate((element) => element.srcObject?.active === true))
      .toBe(true);
  } finally {
    await Promise.all([listener.context.close(), speaker.context.close()]);
  }
});

test("full reconnect and replacement install new remote handles and server IDs", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(120_000);
  const room = `remote-replacement-${browserName}-${Date.now()}`;
  const teacher = await openParticipant(browser, browserName, baseURL, room, "teacher");
  const student = await openParticipant(browser, browserName, baseURL, room, "student");

  try {
    await student.page.getByRole("button", { name: "Start camera" }).click();
    await student.page.getByRole("button", { name: "Publish camera" }).click();
    const video = remoteMedia(teacher.page, "student", "video");
    await expect(video).toHaveAttribute("data-attached", "true", { timeout: 30_000 });
    const session = teacher.page.locator("#room-session");
    const held = teacher.page.locator("#held-remote-handles");
    await teacher.page.locator("#hold-first-remote-handles").dispatchEvent("click");
    await expect(held).toHaveAttribute("data-participant-terminal", "false");
    await expect(held).toHaveAttribute("data-publication-terminal", "false");
    await expect(held).toHaveAttribute("data-media-terminal", "false");
    await teacher.page.evaluate(() => {
      const participant = window.__remoteMediaRoom.remoteParticipants.get("student");
      const publication = participant.trackPublications.values().next().value;
      window.__preReconnectRemoteObjects = {
        participant,
        publication,
        track: publication.track,
      };
    });
    const preReconnectRevision = Number(await session.getAttribute("data-revision"));
    const originalRoom = await teacher.page.evaluateHandle(() => window.__remoteMediaRoom);

    await teacher.page.evaluate(() => window.__remoteMediaRoom.simulateScenario("full-reconnect"));

    await expect(teacher.page.locator("#connection-state")).toHaveText("connected", {
      timeout: 30_000,
    });
    await expect
      .poll(async () => Number(await session.getAttribute("data-revision")), { timeout: 30_000 })
      .toBeGreaterThan(preReconnectRevision);
    await expect(session).toHaveAttribute("data-terminal", "false");
    expect(
      await teacher.page.evaluate((room) => room === window.__remoteMediaRoom, originalRoom),
    ).toBe(true);
    await expect
      .poll(
        () =>
          teacher.page.evaluate(() => {
            const participant = window.__remoteMediaRoom.remoteParticipants.get("student");
            const publication = participant?.trackPublications.values().next().value;
            if (!publication?.track) return null;
            return {
              participant: participant !== window.__preReconnectRemoteObjects.participant,
              publication: publication !== window.__preReconnectRemoteObjects.publication,
              track: publication.track !== window.__preReconnectRemoteObjects.track,
            };
          }),
        { timeout: 30_000 },
      )
      .toEqual({ participant: true, publication: true, track: true });
    await expect(held).toHaveAttribute("data-participant-terminal", "true");
    await expect(held).toHaveAttribute("data-publication-terminal", "true");
    await expect(held).toHaveAttribute("data-media-terminal", "true");
    await expect(video).toHaveAttribute("data-attached", "true");
    await expect
      .poll(() => video.locator("video").evaluate((element) => element.srcObject?.active === true))
      .toBe(true);

    const snapshotParticipant = teacher.page.locator(
      '.remote-participant[data-identity="student"]',
    );
    const snapshotPublication = snapshotParticipant.locator(
      '.remote-track-publication[data-kind="video"]',
    );
    const participantSid = await snapshotParticipant.getAttribute("data-participant-sid");
    const publicationSid = await snapshotPublication.getAttribute("data-publication-sid");
    const previousRevision = Number(
      await teacher.page.locator("#room-session").getAttribute("data-revision"),
    );
    await teacher.page.locator("#observe-room-events").dispatchEvent("click");
    await teacher.page.locator("#hold-first-remote-handles").dispatchEvent("click");
    await expect(held).toHaveAttribute("data-participant-terminal", "false");
    await expect(held).toHaveAttribute("data-publication-terminal", "false");
    await expect(held).toHaveAttribute("data-media-terminal", "false");
    await video.locator("video").evaluate((element) => {
      window.__remoteReplacementOldElement = element;
      window.__remoteReplacementDetachCount = 0;
      const publication = window.__remoteMediaRoom.remoteParticipants
        .get("student")
        .trackPublications.values()
        .next().value;
      const track = publication.track;
      const originalDetach = track.detach.bind(track);
      track.detach = (...args) => {
        window.__remoteReplacementDetachCount += 1;
        return originalDetach(...args);
      };
    });

    const replacementIds = await teacher.page.evaluate(() => {
      const room = window.__remoteMediaRoom;
      const oldParticipant = room.remoteParticipants.get("student");
      const oldPublication = oldParticipant.trackPublications.values().next().value;
      const oldTrack = oldPublication.track;
      const replacementParticipantSid = `${oldParticipant.sid}-replacement`;
      const replacementPublicationSid = `${oldPublication.trackSid}-replacement`;
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
        oldPublication,
        forward(oldPublication, {
          track: replacementTrack,
          trackSid: replacementPublicationSid,
        }),
      );
      const replacementPublications = new Map(oldParticipant.trackPublications);
      replacementPublications.delete(oldPublication.trackSid);
      replacementPublications.set(replacementPublicationSid, replacementPublication);
      const replacementParticipant = new Proxy(
        oldParticipant,
        forward(oldParticipant, {
          sid: replacementParticipantSid,
          trackPublications: replacementPublications,
        }),
      );
      room.remoteParticipants.set(oldParticipant.identity, replacementParticipant);
      window.__remoteReplacementOldParticipant = oldParticipant;
      window.__remoteReplacementOldPublication = oldPublication;
      window.__remoteReplacementParticipant = replacementParticipant;
      room.emit(
        window.LivekitClient.RoomEvent.TrackUnpublished,
        oldPublication,
        oldParticipant,
      );
      return {
        participantSid: replacementParticipantSid,
        publicationSid: replacementPublicationSid,
      };
    });

    await teacher.page.locator("#refresh-room-session").dispatchEvent("click");
    await expect(held).toHaveAttribute("data-media-terminal", "true");
    await expect
      .poll(() => teacher.page.evaluate(() => window.__remoteReplacementDetachCount))
      .toBeGreaterThan(0);
    await teacher.page.evaluate(() => {
      window.__remoteMediaRoom.emit(
        window.LivekitClient.RoomEvent.ParticipantConnected,
        window.__remoteReplacementParticipant,
      );
    });

    expect(replacementIds.participantSid).not.toBe(participantSid);
    expect(replacementIds.publicationSid).not.toBe(publicationSid);
    await expect(snapshotParticipant).toHaveAttribute(
      "data-participant-sid",
      replacementIds.participantSid,
    );
    await expect(snapshotPublication).toHaveAttribute(
      "data-publication-sid",
      replacementIds.publicationSid,
    );
    await expect(held).toHaveAttribute("data-participant-terminal", "true");
    await expect(held).toHaveAttribute("data-publication-terminal", "true");
    await expect(held).toHaveAttribute("data-media-terminal", "true");
    await expect(video).toHaveAttribute("data-attached", "true");
    await expect
      .poll(() => video.locator("video").evaluate((element) => element.srcObject?.active === true))
      .toBe(true);

    const events = teacher.page.locator("#room-events");
    await expect(events).toHaveAttribute("data-event-type", "remoteParticipantJoined");
    await expect(events).toHaveAttribute("data-count", "1");
    const eventRevision = Number(await events.getAttribute("data-event-revision"));
    expect(eventRevision).toBeGreaterThan(previousRevision);
    await expect(events).toHaveAttribute("data-snapshot-revision", String(eventRevision));
    await teacher.page.evaluate(() => {
      const room = window.__remoteMediaRoom;
      const events = window.LivekitClient.RoomEvent;
      room.emit(
        events.TrackMuted,
        window.__remoteReplacementOldPublication,
        window.__remoteReplacementOldParticipant,
      );
      room.emit(
        events.TrackSubscriptionStatusChanged,
        window.__remoteReplacementOldPublication,
        "subscribed",
        window.__remoteReplacementOldParticipant,
      );
    });
    await teacher.page.waitForTimeout(100);
    await expect(events).toHaveAttribute("data-count", "1");
    await expect(session).toHaveAttribute("data-revision", String(eventRevision));

    await teacher.page.locator("#mutate-held-remote-publication").dispatchEvent("click");
    await expect(held).toHaveAttribute("data-mutation", "terminal");
    await expect(video).toHaveAttribute("data-subscribed", "true");
    await expect(teacher.page.locator(".notice.error")).toHaveCount(0);

    await teacher.page.getByRole("button", { name: "Leave room" }).click();
    await expect(teacher.page.locator("#connection-state")).toHaveText("disconnected");
    await expect(teacher.page.locator("#room-session")).toHaveAttribute("data-terminal", "true");
    const terminalRevision = await teacher.page
      .locator("#room-session")
      .getAttribute("data-revision");
    await teacher.page.evaluate(() => {
      const room = window.__remoteMediaRoom;
      room.state = "connected";
      room.emit(window.LivekitClient.RoomEvent.ConnectionStateChanged, room.state);
    });
    await teacher.page.locator("#refresh-room-session").dispatchEvent("click");
    await expect(teacher.page.locator("#room-session")).toHaveAttribute(
      "data-revision",
      terminalRevision,
    );
    await expect(teacher.page.locator("#room-session")).toHaveAttribute("data-terminal", "true");
  } finally {
    await Promise.all([teacher.context.close(), student.context.close()]);
  }
});
