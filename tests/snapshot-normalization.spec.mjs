import { expect, test } from "@playwright/test";

const REMOTE_PARTICIPANTS = 99;
const PUBLICATIONS_PER_PARTICIPANT = 2;
const WARM_UP_SAMPLES = 3;
const MEASURED_SAMPLES = 15;
const P95_BUDGET_MS = 250;

async function exposeRoom(page) {
  await page.evaluate(() => {
    const OriginalRoom = globalThis.LivekitClient.Room;
    const InstrumentedRoom = new Proxy(OriginalRoom, {
      construct(target, args, newTarget) {
        const room = Reflect.construct(target, args, newTarget);
        globalThis.__snapshotProfileRoom = room;
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

async function installTargetScaleFixture(page) {
  await page.evaluate((participantCount) => {
    const room = globalThis.__snapshotProfileRoom;
    for (let index = 0; index < participantCount; index += 1) {
      const suffix = String(index).padStart(3, "0");
      const identity = `profile-${suffix}`;
      const publications = new Map();
      for (const [kind, source] of [
        ["audio", "microphone"],
        ["video", "camera"],
      ]) {
        const sid = `TR_${suffix}_${kind}`;
        publications.set(sid, {
          isMuted: false,
          isSubscribed: false,
          kind,
          source,
          track: null,
          trackName: `${source}-${suffix}`,
          trackSid: sid,
        });
      }
      room.remoteParticipants.set(identity, {
        attributes: { cohort: "target-scale", index: suffix },
        connectionQuality: "good",
        dataTracks: new Map(),
        identity,
        metadata: `participant-${suffix}`,
        sid: `PA_${suffix}`,
        trackPublications: publications,
      });
    }
    Object.defineProperty(room, "metadata", {
      configurable: true,
      value: "profile-fixture-ready",
    });
    room.emit(globalThis.LivekitClient.RoomEvent.RoomMetadataChanged, room.metadata);
  }, REMOTE_PARTICIPANTS);

  await expect(page.locator("#room-session")).toHaveAttribute(
    "data-remote-participant-count",
    String(REMOTE_PARTICIPANTS),
  );
  await expect(page.locator(".remote-track-publication")).toHaveCount(
    REMOTE_PARTICIPANTS * PUBLICATIONS_PER_PARTICIPANT,
  );
}

async function measureCoalescedSnapshot(page, sample) {
  return page.evaluate(async (sequence) => {
    const room = globalThis.__snapshotProfileRoom;
    const session = document.querySelector("#room-session");
    const events = document.querySelector("#room-events");
    const previousRevision = Number(session.dataset.revision);
    const previousEventCount = Number(events.dataset.count);
    const metadata = `profile-sample-${sequence}`;
    const startedAt = performance.now();

    const completion = new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        observer.disconnect();
        reject(new Error(`snapshot profile sample ${sequence} timed out`));
      }, 5_000);
      const observer = new MutationObserver(() => {
        const revision = Number(session.dataset.revision);
        const eventCount = Number(events.dataset.count);
        if (
          revision !== previousRevision + 1 ||
          eventCount !== previousEventCount + 2 ||
          events.dataset.eventRevision !== String(revision) ||
          events.dataset.snapshotRevision !== String(revision) ||
          session.dataset.roomMetadata !== metadata
        ) {
          return;
        }
        clearTimeout(timeout);
        observer.disconnect();
        requestAnimationFrame(() =>
          resolve({ durationMs: performance.now() - startedAt, revision }),
        );
      });
      observer.observe(document.documentElement, {
        attributeFilter: [
          "data-count",
          "data-event-revision",
          "data-revision",
          "data-room-metadata",
          "data-snapshot-revision",
        ],
        attributes: true,
        subtree: true,
      });
    });

    Object.defineProperty(room, "metadata", {
      configurable: true,
      value: metadata,
    });
    const event = globalThis.LivekitClient.RoomEvent.RoomMetadataChanged;
    room.emit(event, metadata);
    room.emit(event, metadata);
    return completion;
  }, sample);
}

function percentile(sorted, percentage) {
  return sorted[Math.ceil(sorted.length * percentage) - 1];
}

test("coalesced full snapshots meet the initial target near 100 participants", async ({
  browserName,
  page,
}, testInfo) => {
  test.setTimeout(90_000);
  await page.goto("/");
  await exposeRoom(page);
  await page.getByLabel("Room name").fill(`snapshot-profile-${browserName}-${Date.now()}`);
  await page.getByRole("button", { name: "Join room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("connected", { timeout: 30_000 });
  await page.locator("#observe-room-events").dispatchEvent("click");
  await expect(page.locator("#room-events")).toHaveAttribute("data-observing", "true");
  await installTargetScaleFixture(page);

  for (let sample = 0; sample < WARM_UP_SAMPLES; sample += 1) {
    await measureCoalescedSnapshot(page, `warm-${sample}`);
  }
  const samples = [];
  for (let sample = 0; sample < MEASURED_SAMPLES; sample += 1) {
    const result = await measureCoalescedSnapshot(page, sample);
    samples.push(result.durationMs);
  }

  const sorted = samples.toSorted((left, right) => left - right);
  const p50 = percentile(sorted, 0.5);
  const p95 = percentile(sorted, 0.95);
  const maximum = sorted.at(-1);
  await expect(page.locator("#room-session")).toHaveAttribute(
    "data-remote-participant-count",
    String(REMOTE_PARTICIPANTS),
  );
  await expect(page.locator(".remote-track-publication")).toHaveCount(
    REMOTE_PARTICIPANTS * PUBLICATIONS_PER_PARTICIPANT,
  );
  const result = `${browserName}: p50=${p50.toFixed(1)}ms p95=${p95.toFixed(1)}ms max=${maximum.toFixed(1)}ms`;
  testInfo.annotations.push({ type: "snapshot-profile", description: result });
  console.log(`snapshot normalization profile ${result}`);
  expect(p95).toBeLessThanOrEqual(P95_BUDGET_MS);

  await page.evaluate(() => {
    const room = globalThis.__snapshotProfileRoom;
    for (const identity of room.remoteParticipants.keys()) {
      if (identity.startsWith("profile-")) room.remoteParticipants.delete(identity);
    }
    Object.defineProperty(room, "metadata", {
      configurable: true,
      value: "profile-fixture-removed",
    });
    room.emit(globalThis.LivekitClient.RoomEvent.RoomMetadataChanged, room.metadata);
  });
  await expect(page.locator("#room-session")).toHaveAttribute(
    "data-remote-participant-count",
    "0",
  );
  await expect(page.locator(".remote-track-publication")).toHaveCount(0);

  await page.getByRole("button", { name: "Leave room" }).click();
  await expect(page.locator("#connection-state")).toHaveText("disconnected");
});
