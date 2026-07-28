import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant.mjs";

function remoteVideo(page, identity) {
  return page.locator(
    `.remote-media-publication[data-participant-identity="${identity}"][data-kind="video"]`,
  );
}

test("publisher permissions allow and deny media by Participant Identity", async ({
  browser,
  browserName,
  baseURL,
}) => {
  test.setTimeout(90_000);
  const room = `publisher-permissions-${browserName}-${Date.now()}`;
  const teacher = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "teacher",
  });
  const student = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "student",
  });

  try {
    await teacher.page.getByRole("button", { name: "Start camera" }).click();
    await expect(teacher.page.locator("#camera-state")).toHaveText("on");
    await teacher.page.getByRole("button", { name: "Publish camera" }).click();
    await expect(teacher.page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(1);

    const video = remoteVideo(student.page, "teacher");
    await expect(video).toHaveAttribute("data-media-available", "true", { timeout: 20_000 });

    await teacher.page.getByLabel("Allowed participant identity").fill("someone-else");
    await teacher.page.getByRole("button", { name: "Allow only participant" }).click();
    await expect(teacher.page.locator("#publisher-permissions")).toHaveAttribute(
      "data-state",
      "participantSpecific",
    );
    await expect(video).toHaveAttribute("data-media-available", "false", { timeout: 20_000 });
    await expect(video).toHaveAttribute("data-subscribed", "false");
    await expect(student.page.locator('.remote-track-publication[data-kind="video"]')).toHaveCount(
      1,
    );
    await expect(teacher.page.locator("#camera-state")).toHaveText("on");
    await expect(teacher.page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(1);

    await teacher.page.getByLabel("Allowed participant identity").fill("student");
    await teacher.page.getByRole("button", { name: "Allow only participant" }).click();
    await expect(video).toHaveAttribute("data-media-available", "true", { timeout: 20_000 });
    await expect(video).toHaveAttribute("data-subscribed", "true");

    await teacher.page.getByLabel("Allowed participant identity").fill("someone-else");
    await teacher.page.getByRole("button", { name: "Allow only participant" }).click();
    await expect(video).toHaveAttribute("data-media-available", "false", { timeout: 20_000 });
    await teacher.page.getByRole("button", { name: "Allow all participants" }).click();
    await expect(teacher.page.locator("#publisher-permissions")).toHaveAttribute(
      "data-state",
      "allParticipants",
    );
    await expect(video).toHaveAttribute("data-media-available", "true", { timeout: 20_000 });
  } finally {
    await Promise.all([teacher.context.close(), student.context.close()]);
  }
});

test("LiveKit publisher grants deny mutations without stopping local capture", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `publisher-grants-${browserName}-${Date.now()}`;
  const participant = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "publisher-permissions-denied",
  });

  try {
    const session = participant.page.locator("#room-session");
    await expect(session).toHaveAttribute("data-can-publish", "false");

    await participant.page.getByRole("button", { name: "Start camera" }).click();
    await expect(participant.page.locator("#camera-state")).toHaveText("on");
    await participant.page.getByRole("button", { name: "Publish camera" }).click();
    await expect(participant.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    await expect(participant.page.locator('.local-track-publication[data-kind="video"]')).toHaveCount(
      0,
    );
    await expect(participant.page.locator("#camera-state")).toHaveText("on");

    await participant.page.getByLabel("Allowed participant identity").fill("student");
    await participant.page.getByRole("button", { name: "Allow only participant" }).click();
    await expect(participant.page.locator("#media-operation-error")).toHaveAttribute(
      "data-kind",
      "permissionDenied",
    );
    await expect(participant.page.locator(".notice.error")).toContainText(
      "LiveKit source grants do not permit publishing",
    );
    await expect(participant.page.locator("#publisher-permissions")).toHaveAttribute(
      "data-state",
      "allParticipants",
    );
    await expect(participant.page.locator("#camera-state")).toHaveText("on");
  } finally {
    await participant.context.close();
  }
});
