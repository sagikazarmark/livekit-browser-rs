import { expect, test } from "@playwright/test";
import { createHmac } from "node:crypto";
import { DataPacket, DataPacket_Kind, Transcription, TranscriptionSegment } from "@livekit/protocol";
import { openRoomParticipant } from "./helpers/room-participant";

function base64Url(value) {
  return Buffer.from(value).toString("base64url");
}

function createAgentToken(room, identity) {
  const now = Math.floor(Date.now() / 1_000);
  const header = base64Url(JSON.stringify({ alg: "HS256", typ: "JWT" }));
  const payload = base64Url(
    JSON.stringify({
      exp: now + 3_600,
      iss: process.env.LIVEKIT_API_KEY || "devkey",
      nbf: now - 5,
      sub: identity,
      name: identity,
      video: {
        roomJoin: true,
        room,
        canPublish: true,
        canSubscribe: true,
        canPublishData: true,
        agent: true,
      },
    }),
  );
  const signature = createHmac("sha256", process.env.LIVEKIT_API_SECRET || "secret")
    .update(`${header}.${payload}`)
    .digest("base64url");
  return `${header}.${payload}.${signature}`;
}

async function exposeLiveKitRoom(page) {
  await page.evaluate(() => {
    const OriginalRoom = globalThis.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args) {
        const room = Reflect.construct(target, args);
        globalThis.__transcriptionTestRoom = room;
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

function encodeTranscription(trackId, text, final, sequence) {
  return Array.from(
    new DataPacket({
      kind: DataPacket_Kind.RELIABLE,
      sequence,
      destinationIdentities: ["transcription-receiver"],
      value: {
        case: "transcription",
        value: new Transcription({
          transcribedParticipantIdentity: "transcription-source",
          trackId,
          segments: [
            new TranscriptionSegment({
              id: "segment-1",
              text,
              language: "en-US",
              startTime: 120n,
              endTime: 540n,
              final,
            }),
          ],
        }),
      },
    }).toBinary(),
  );
}

async function sendTranscription(page, bytes) {
  await page.evaluate(async (encoded) => {
    const packet = {
      sequence: 0,
      value: { case: "transcription" },
      toBinary: () => Uint8Array.from(encoded),
    };
    await globalThis.__transcriptionTestRoom.engine.sendDataPacket(
      packet,
      globalThis.LivekitClient.DataPacket_Kind.RELIABLE,
    );
  }, bytes);
}

async function emitTranscriptionWithoutResolvedSource(page) {
  await page.evaluate(() => {
    globalThis.__transcriptionTestRoom.emit(
      globalThis.LivekitClient.RoomEvent.TranscriptionReceived,
      [
        {
          id: "segment-without-source",
          text: "source unavailable",
          language: "",
          startTime: 0,
          endTime: 0,
          final: false,
          firstReceivedTime: 1_721_234_567_080,
          lastReceivedTime: 1_721_234_567_080,
        },
      ],
      undefined,
      undefined,
    );
  });
}

async function emitStaleTranscriptionCallback(page) {
  await page.evaluate(() => {
    const source = globalThis.__transcriptionTestSource;
    globalThis.__transcriptionTestRoom.emit(
      globalThis.LivekitClient.RoomEvent.TranscriptionReceived,
      [
        {
          id: "ended-source",
          text: "ended source",
          language: "en-US",
          startTime: 0,
          endTime: 1,
          final: true,
          firstReceivedTime: Date.now(),
          lastReceivedTime: Date.now(),
        },
      ],
      source.participant,
      source.publication,
    );
  });
}

test("transcription segments replace interim text and clean up with their source and Room Session", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `transcription-${browserName}-${Date.now()}`;
  const source = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "transcription-source",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "transcription-receiver",
    prepare: exposeLiveKitRoom,
  });
  const emitter = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "transcription-emitter",
    prepare: exposeLiveKitRoom,
    participantToken: createAgentToken(room, "transcription-emitter"),
  });

  try {
    const transcriptions = receiver.page.locator("#transcription-segments");
    await expect(source.page.locator("#participant-count")).toHaveText("3");
    await expect(receiver.page.locator("#participant-count")).toHaveText("3");
    await expect(transcriptions).toHaveAttribute("data-observing", "true");

    await source.page.getByRole("button", { name: "Start microphone" }).click();
    await source.page.getByRole("button", { name: "Publish microphone" }).click();
    await expect
      .poll(() =>
        receiver.page.evaluate(() => {
          const participant = globalThis.__transcriptionTestRoom.remoteParticipants.get(
            "transcription-source",
          );
          return [...(participant?.trackPublications.values() || [])].some(
            (publication) =>
              publication.source === globalThis.LivekitClient.Track.Source.Microphone,
          );
        }),
      )
      .toBe(true);

    await receiver.page.locator("#observe-room-events").dispatchEvent("click");
    await expect(receiver.page.locator("#room-events")).toHaveAttribute(
      "data-observing",
      "true",
    );
    const roomEventCount = await receiver.page
      .locator("#room-events")
      .getAttribute("data-count");

    const trackSid = await receiver.page.evaluate(() => {
      const participant = globalThis.__transcriptionTestRoom.remoteParticipants.get(
        "transcription-source",
      );
      const publication = [...participant.trackPublications.values()].find(
        (candidate) => candidate.source === globalThis.LivekitClient.Track.Source.Microphone,
      );
      globalThis.__transcriptionTestSource = { participant, publication };
      return publication.trackSid;
    });
    await sendTranscription(
      emitter.page,
      encodeTranscription(trackSid, "hello wor", false, 1),
    );
    await sendTranscription(
      emitter.page,
      encodeTranscription(trackSid, "hello world", true, 2),
    );

    await expect(transcriptions).toHaveAttribute("data-count", "2");
    await expect(transcriptions).toHaveAttribute("data-ids", "segment-1|segment-1");
    await expect(transcriptions).toHaveAttribute("data-texts", "hello wor|hello world");
    await expect(transcriptions).toHaveAttribute(
      "data-participants",
      "transcription-source|transcription-source",
    );
    await expect(transcriptions).toHaveAttribute(
      "data-track-publication-sids",
      `${trackSid}|${trackSid}`,
    );
    await expect(transcriptions).toHaveAttribute("data-start-times", "120|120");
    await expect(transcriptions).toHaveAttribute("data-end-times", "540|540");
    await expect(transcriptions).toHaveAttribute("data-languages", "en-US|en-US");
    await expect(transcriptions).toHaveAttribute("data-finals", "false|true");
    await expect(transcriptions).toHaveAttribute(
      "data-first-received-times",
      /^(\d+)\|\1$/,
    );
    const receivedTimes = (await transcriptions.getAttribute("data-last-received-times"))
      .split("|")
      .map(Number);
    expect(receivedTimes[1]).toBeGreaterThanOrEqual(receivedTimes[0]);
    await expect(transcriptions).toHaveAttribute("data-dropped", "0");
    expect(await receiver.page.locator("#room-events").getAttribute("data-count")).toBe(
      roomEventCount,
    );

    await emitTranscriptionWithoutResolvedSource(receiver.page);
    await expect(transcriptions).toHaveAttribute("data-count", "3");
    await expect(transcriptions).toHaveAttribute(
      "data-participants",
      "transcription-source|transcription-source|",
    );
    await expect(transcriptions).toHaveAttribute(
      "data-track-publication-sids",
      `${trackSid}|${trackSid}|`,
    );

    await source.page.getByRole("button", { name: "Unpublish microphone" }).click();
    await expect
      .poll(() =>
        receiver.page.evaluate(() => {
          const participant = globalThis.__transcriptionTestRoom.remoteParticipants.get(
            "transcription-source",
          );
          return {
            participantCurrent:
              participant === globalThis.__transcriptionTestSource.participant,
            microphonePublished: [...(participant?.trackPublications.values() || [])].some(
              (publication) =>
                publication.source === globalThis.LivekitClient.Track.Source.Microphone,
            ),
          };
        }),
      )
      .toEqual({ participantCurrent: true, microphonePublished: false });
    await expect(receiver.page.locator("#participant-count")).toHaveText("3");
    await emitStaleTranscriptionCallback(receiver.page);
    await expect(transcriptions).toHaveAttribute("data-count", "3");

    await source.page.getByRole("button", { name: "Leave room" }).click();
    await expect(receiver.page.locator("#participant-count")).toHaveText("2");
    await emitStaleTranscriptionCallback(receiver.page);
    await expect(transcriptions).toHaveAttribute("data-count", "3");

    await receiver.page.getByRole("button", { name: "Leave room" }).click();
    await expect(transcriptions).toHaveAttribute("data-closed", "true");
  } finally {
    await Promise.all([source.context.close(), receiver.context.close(), emitter.context.close()]);
  }
});
