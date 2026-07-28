import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant";

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

test("Remote Participants and Track Publications remain coherent room state", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `remote-state-${Date.now()}`;
  const studentIdentity = "__proto__";
  const teacher = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "teacher",
    prepare: exposeRoom,
  });
  await teacher.page.locator("#observe-room-events").dispatchEvent("click");
  const teacherRevision = Number(
    await teacher.page.locator("#room-session").getAttribute("data-revision"),
  );

  const student = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: studentIdentity,
    prepare: exposeRoom,
  });

  const observedStudent = teacher.page.locator(
    `.remote-participant[data-identity="${studentIdentity}"]`,
  );
  const observedTeacher = student.page.locator(
    '.remote-participant[data-identity="teacher"]',
  );
  await expect(observedStudent).toHaveCount(1);
  await expect(observedStudent).toHaveAttribute("data-participant-sid", /^PA_/);
  await expect(observedTeacher).toHaveCount(1);
  await expect(observedTeacher).toHaveAttribute("data-participant-sid", /^PA_/);
  await expect(teacher.page.locator("#room-events")).toHaveAttribute(
    "data-event-type",
    "remoteParticipantJoined",
  );
  await expect(teacher.page.locator("#room-events")).toHaveAttribute(
    "data-event-identity",
    studentIdentity,
  );

  await student.page.evaluate(async () => {
    const track = await globalThis.LivekitClient.createLocalVideoTrack();
    globalThis.__livekitTestVideoTrack = track;
    globalThis.__livekitTestVideoPublication =
      await globalThis.__livekitTestRoom.localParticipant.publishTrack(track);
  });
  const camera = observedStudent.locator(
    '.remote-track-publication[data-kind="video"]',
  );
  await expect(camera).toHaveCount(1);
  await expect(camera).toHaveAttribute("data-publication-sid", /^TR_/);
  await expect(camera).toHaveAttribute("data-muted", "false");
  await expect(camera).toHaveAttribute("data-subscribed", "true");
  await expect(camera).toHaveAttribute("data-media-available", "true");

  await student.page.evaluate(async () => {
    const track = await globalThis.LivekitClient.createLocalAudioTrack();
    globalThis.__livekitTestTrack = track;
    globalThis.__livekitTestPublication =
      await globalThis.__livekitTestRoom.localParticipant.publishTrack(track);
  });

  const microphone = observedStudent.locator(
    '.remote-track-publication[data-kind="audio"]',
  );
  await expect(microphone).toHaveCount(1);
  await expect(microphone).toHaveAttribute("data-publication-sid", /^TR_/);
  await expect(microphone).toHaveAttribute("data-muted", "false");
  await expect(microphone).toHaveAttribute("data-subscribed", "true");
  await expect(microphone).toHaveAttribute("data-media-available", "true");
  await expect(teacher.page.locator("#room-events")).toHaveAttribute(
    "data-event-types",
    /remoteTrackPublicationsChanged/,
  );
  await expect(teacher.page.locator("#room-events")).toHaveAttribute(
    "data-event-identity",
    studentIdentity,
  );

  await student.page.evaluate(async () => {
    globalThis.__livekitTestDataTrack =
      await globalThis.__livekitTestRoom.localParticipant.publishDataTrack({
        name: "reactions",
      });
  });
  const dataTrackState = await teacher.page.evaluate((identity) => {
    const participant = globalThis.__livekitTestRoom.remoteParticipants.get(identity);
    const track = participant.dataTracks.get("reactions");
    return {
      sid: track.info.sid,
      name: track.info.name,
      isDataTrack: track instanceof globalThis.LivekitClient.RemoteDataTrack,
      isPublication: track instanceof globalThis.LivekitClient.RemoteTrackPublication,
      inPublications: participant.trackPublications.has(track.info.sid),
      hasSetSubscribed: typeof track.setSubscribed === "function",
    };
  }, studentIdentity);
  expect(dataTrackState).toMatchObject({
    name: "reactions",
    isDataTrack: true,
    isPublication: false,
    inPublications: false,
    hasSetSubscribed: false,
  });
  const dataTrack = observedStudent.locator(
    `.remote-data-track[data-track-sid="${dataTrackState.sid}"]`,
  );
  await expect(dataTrack).toHaveCount(1);
  await expect(dataTrack).toHaveAttribute("data-name", "reactions");
  await expect(
    observedStudent.locator(
      `.remote-track-publication[data-publication-sid="${dataTrackState.sid}"]`,
    ),
  ).toHaveCount(0);
  await expect(teacher.page.locator("#room-events")).toHaveAttribute(
    "data-event-types",
    /remoteDataTracksChanged/,
  );
  await expect(teacher.page.locator("#room-events")).toHaveAttribute(
    "data-snapshot-revision",
    await teacher.page.locator("#room-session").getAttribute("data-revision"),
  );

  await student.page.evaluate(() => globalThis.__livekitTestPublication.mute());
  await expect(microphone).toHaveAttribute("data-muted", "true");
  await expect(microphone).toHaveCount(1);

  await teacher.page.evaluate((identity) => {
    const publication = globalThis.__livekitTestRoom.remoteParticipants
      .get(identity)
      .audioTrackPublications.values()
      .next().value;
    publication.setSubscribed(false);
  }, studentIdentity);
  await expect(microphone).toHaveAttribute("data-subscribed", "false");
  await expect(microphone).toHaveAttribute("data-media-available", "false");
  await expect(microphone).toHaveCount(1);

  const events = teacher.page.locator("#room-events");
  const eventCount = Number(await events.getAttribute("data-count"));
  const revision = Number(
    await teacher.page.locator("#room-session").getAttribute("data-revision"),
  );
  await teacher.page.evaluate(async (identity) => {
    const participant = globalThis.__livekitTestRoom.remoteParticipants.get(identity);
    const event = globalThis.LivekitClient.RoomEvent;
    participant.metadata = "preparing";
    globalThis.__livekitTestRoom.emit(event.ParticipantMetadataChanged, "", participant);
    await Promise.resolve();
    participant.metadata = "ready";
    globalThis.__livekitTestRoom.emit(
      event.ParticipantMetadataChanged,
      "preparing",
      participant,
    );
  }, studentIdentity);

  await expect(events).toHaveAttribute("data-count", String(eventCount + 2));
  await expect(teacher.page.locator("#room-session")).toHaveAttribute(
    "data-revision",
    String(revision + 1),
  );
  await expect(events).toHaveAttribute("data-event-revision", String(revision + 1));
  await expect(events).toHaveAttribute("data-snapshot-revision", String(revision + 1));
  await expect(observedStudent).toHaveAttribute("data-metadata", "ready");
  expect(revision).toBeGreaterThan(teacherRevision);

  await student.page.evaluate(async () => {
    await globalThis.__livekitTestDataTrack.unpublish();
    await globalThis.__livekitTestRoom.localParticipant.unpublishTrack(
      globalThis.__livekitTestTrack,
      true,
    );
    await globalThis.__livekitTestRoom.localParticipant.unpublishTrack(
      globalThis.__livekitTestVideoTrack,
      true,
    );
  });
  await expect(dataTrack).toHaveCount(0);
  await Promise.all([teacher.context.close(), student.context.close()]);
});
