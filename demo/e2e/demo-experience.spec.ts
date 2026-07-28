import { expect, test } from "@playwright/test";
import { openRoomParticipant } from "./helpers/room-participant";

test("prefills a second participant and explains browser capabilities", async ({ page }) => {
  await page.goto("/?room=demo-room&identity=demo-primary");

  await expect(page.getByLabel("Room name")).toHaveValue("demo-room");
  await expect(page.getByLabel("Your identity")).toHaveValue("demo-primary");
  await expect(page.locator("#capability-panel")).toHaveAttribute("data-ready", "true");
  await expect(page.locator("#capability-panel .capability-item")).toHaveCount(8);
  await expect(page.locator('#lifecycle-guide [data-step="runtime"]')).toHaveAttribute(
    "data-complete",
    "true",
  );

  const secondParticipantHref = await page
    .getByRole("link", { name: "Open second participant" })
    .getAttribute("href");
  const secondParticipantUrl = new URL(secondParticipantHref);
  expect(secondParticipantUrl.searchParams.get("room")).toBe("demo-room");
  expect(secondParticipantUrl.searchParams.get("identity")).not.toBe("demo-primary");

  await page.goto(secondParticipantHref);
  await expect(page.getByLabel("Room name")).toHaveValue("demo-room");
  await expect(page.getByLabel("Your identity")).toHaveValue(
    secondParticipantUrl.searchParams.get("identity"),
  );
});

test("Chat Messages and media lifecycles advance the guided demo", async ({
  browser,
  browserName,
  baseURL,
}) => {
  const room = `demo-chat-${browserName}-${Date.now()}`;
  const sender = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "demo-sender",
  });
  const receiver = await openRoomParticipant({
    browser,
    browserName,
    baseURL,
    room,
    identity: "demo-receiver",
  });

  try {
    for (const participant of [sender, receiver]) {
      await expect(participant.page.locator('#lifecycle-guide [data-step="joined"]')).toHaveAttribute(
        "data-complete",
        "true",
      );
      await expect(
        participant.page.locator('#lifecycle-guide [data-step="remote-participant"]'),
      ).toHaveAttribute("data-complete", "true");
    }

    await sender.page.locator("#chat-message-input").fill("Hello from the visible demo");
    await sender.page.locator("#send-chat-message").click();

    await expect(receiver.page.locator("#demo-chat .chat-message")).toContainText(
      "Hello from the visible demo",
    );
    await expect(receiver.page.locator("#demo-chat .chat-message")).toContainText("demo-sender");
    await expect(receiver.page.locator('#lifecycle-guide [data-step="chat"]')).toHaveAttribute(
      "data-complete",
      "true",
    );

    await sender.page.getByRole("button", { name: "Start camera" }).click();
    await expect(sender.page.locator('#lifecycle-guide [data-step="capture"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await sender.page.getByRole("button", { name: "Publish camera" }).click();
    await expect(sender.page.locator('#lifecycle-guide [data-step="publish"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await sender.page.getByRole("button", { name: "Unpublish camera" }).click();
    await expect(sender.page.locator('#lifecycle-guide [data-step="unpublish"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await expect(sender.page.locator('#lifecycle-guide [data-step="publish"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await sender.page.getByRole("button", { name: "Stop camera" }).click();
    await expect(sender.page.locator('#lifecycle-guide [data-step="stop"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await expect(sender.page.locator('#lifecycle-guide [data-step="capture"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await sender.page.getByRole("button", { name: "Leave room" }).click();
    await expect(sender.page.locator('#lifecycle-guide [data-step="leave"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
    await expect(sender.page.locator('#lifecycle-guide [data-step="joined"]')).toHaveAttribute(
      "data-complete",
      "true",
    );
  } finally {
    await Promise.all([sender.context.close(), receiver.context.close()]);
  }
});
