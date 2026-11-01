import { expect, Page, test } from "@playwright/test";

// Requires a running server (mock provider configured + user bob) and the mock
// LLM at :9999. Seed via E2E_USER/E2E_PASS; chat is seeded through the API so
// the test exercises login → send → streamed reply in the UI itself.
const USER = process.env.E2E_USER ?? "bob";
const PASS = process.env.E2E_PASS ?? "bobpassword";

async function login(page: Page) {
  await page.goto("/");
  await page.locator('input[autocomplete="username"]').fill(USER);
  await page.locator('input[autocomplete="current-password"]').fill(PASS);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("button", { name: "New chat" })).toBeVisible();
}

test("login, chat round-trip vs mock, perf badge", async ({ page }) => {
  await login(page);

  const { providers } = (await (await page.request.get("/api/providers")).json()) as {
    providers: { id: string; name: string }[];
  };
  const provider = providers[0];
  const title = `playwright-e2e-${Date.now()}`;
  const chat = (
    await (
      await page.request.post("/api/chats", {
        data: { title, provider_id: provider.id, model: "mock-gpt" },
      })
    ).json()
  ) as { id: string };

  await page.reload();
  await page.locator(`nav div[title="${title}"]`).click();
  // selectChat awaits getChat before setActiveId — until the header shows the
  // title, the composer still belongs to no chat (a send would open a new one).
  await expect(page.getByRole("heading", { name: title })).toBeVisible();

  const composer = page.locator("textarea");
  await composer.click();
  // "SLOW" makes the mock chunk with 1.2s delays — keeps the perf badge on
  // screen long enough for the assertion (badge unmounts when streaming ends).
  await composer.fill("SLOW hello from playwright");
  await composer.press("Enter");

  await expect(page.getByText(/first \d+\.\d+s/)).toBeVisible({ timeout: 15_000 });
  await expect(page.getByText(/Hello from mock!/)).toBeVisible({ timeout: 60_000 });
  await page.request.delete(`/api/chats/${chat.id}`);
});

test("command palette: Ctrl+K → new chat action", async ({ page }) => {
  await login(page);
  await page.keyboard.press("Control+k");
  const input = page.getByPlaceholder("Search chats, models, actions…");
  await expect(input).toBeVisible();
  await input.fill("New chat");
  await page.keyboard.press("Enter");
  await expect(page.getByRole("heading", { name: "New chat" })).toBeVisible();
});

test("settings drawer opens and closes", async ({ page }) => {
  await login(page);
  await page.getByRole("button", { name: "Open settings" }).first().click();
  const close = page.getByRole("button", { name: "Close settings" });
  await expect(close).toBeVisible();
  await close.click();
});

test("auto-scroll follows output only while pinned; scrolling up unlocks", async ({ page }) => {
  await login(page);
  const { providers } = (await (await page.request.get("/api/providers")).json()) as {
    providers: { id: string }[];
  };
  const chat = (
    await (
      await page.request.post("/api/chats", {
        data: { title: "playwright-scroll", provider_id: providers[0].id, model: "mock-gpt" },
      })
    ).json()
  ) as { id: string };

  await page.reload();
  await page.locator('nav div[title="playwright-scroll"]').click();
  await expect(page.getByRole("heading", { name: "playwright-scroll" })).toBeVisible();
  const composer = page.locator("textarea");
  const filler = Array.from({ length: 80 }, (_, i) => `filler line ${i}`).join("\n\n");
  await composer.fill(`SLOW ${filler}`);
  await composer.press("Enter");

  const scroller = page.getByTestId("message-scroll");
  const jump = page.getByRole("button", { name: /Jump to latest/ });
  await page.waitForTimeout(300);
  await scroller.evaluate((el) => el.scrollTo(0, 0));
  await expect(jump).toBeVisible({ timeout: 3000 });

  // More chunks arrive (mock streams ~1.2s apart) but the view must stay put.
  await page.waitForTimeout(2600);
  const scrollTop = await scroller.evaluate((el) => el.scrollTop);
  expect(scrollTop).toBeLessThan(60);

  await jump.click();
  await expect(jump).toBeHidden({ timeout: 3000 });

  await page.request.delete(`/api/chats/${chat.id}`);
});

test("typing while a reply streams queues the next message", async ({ page }) => {
  await login(page);
  const { providers } = (await (await page.request.get("/api/providers")).json()) as {
    providers: { id: string }[];
  };
  const title = `pw-queue-${Date.now()}`;
  const chat = (
    await (
      await page.request.post("/api/chats", {
        data: { title, provider_id: providers[0].id, model: "mock-gpt" },
      })
    ).json()
  ) as { id: string };

  await page.reload();
  await page.locator(`nav div[title="${title}"]`).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  const composer = page.locator("textarea");
  await composer.fill("SLOW first question");
  await composer.press("Enter");
  await expect(page.getByText(/first \d+\.\d+s/)).toBeVisible({ timeout: 20_000 });

  // Composer stays usable; Enter while streaming queues instead of sending.
  await composer.fill("second queued message");
  await composer.press("Enter");
  await expect(page.getByText(/1 queued/)).toBeVisible();
  await expect(page.getByTestId("message-scroll").getByText("second queued message")).toBeVisible({
    timeout: 60_000,
  });
  await expect(page.getByText(/1 queued/)).toBeHidden({ timeout: 30_000 });
  await page.request.delete(`/api/chats/${chat.id}`);
});

test("a second viewer receives the reply live (multi-device sync)", async ({ browser }) => {
  const ctxA = await browser.newContext();
  const pageA = await ctxA.newPage();
  await login(pageA);
  const { providers } = (await (await pageA.request.get("/api/providers")).json()) as {
    providers: { id: string }[];
  };
  const title = `pw-sync-${Date.now()}`;
  const chat = (
    await (
      await pageA.request.post("/api/chats", {
        data: { title, provider_id: providers[0].id, model: "mock-gpt" },
      })
    ).json()
  ) as { id: string };

  const ctxB = await browser.newContext();
  const pageB = await ctxB.newPage();
  await login(pageB);
  await pageB.locator(`nav div[title="${title}"]`).click();
  await expect(pageB.getByRole("heading", { name: title })).toBeVisible();
  await pageA.reload();
  await pageA.locator(`nav div[title="${title}"]`).click();
  await expect(pageA.getByRole("heading", { name: title })).toBeVisible();

  const composer = pageA.locator("textarea");
  await composer.fill("SLOW live-sync probe");
  await composer.press("Enter");
  await expect(pageA.getByText(/Hello from mock!/)).toBeVisible({ timeout: 60_000 });
  // B never sent or reloaded after the send — events subscription only.
  await expect(pageB.getByText(/Hello from mock!/)).toBeVisible({ timeout: 60_000 });
  await expect(
    pageB.getByTestId("message-scroll").getByText("SLOW live-sync probe", { exact: true }),
  ).toBeVisible({ timeout: 60_000 });
  await pageA.request.delete(`/api/chats/${chat.id}`);
  await ctxA.close();
  await ctxB.close();
});

test("theme toggle flips light/dark and persists across reload", async ({ page }) => {
  await login(page);
  const html = page.locator("html");
  // Fresh context + Playwright's light color-scheme → starts light.
  await expect(html).toHaveAttribute("data-theme", "light");
  await page.getByRole("button", { name: "Switch to dark theme" }).click();
  await expect(html).toHaveAttribute("data-theme", "dark");
  await page.reload();
  await expect(html).toHaveAttribute("data-theme", "dark");
  await page.getByRole("button", { name: "Switch to light theme" }).click();
  await expect(html).toHaveAttribute("data-theme", "light");
});

test("send-status ticks: queued pending row resolves to delivered check", async ({ page }) => {
  await login(page);
  const { providers } = (await (await page.request.get("/api/providers")).json()) as {
    providers: { id: string }[];
  };
  const title = `pw-ticks-${Date.now()}`;
  const chat = (
    await (
      await page.request.post("/api/chats", {
        data: { title, provider_id: providers[0].id, model: "mock-gpt" },
      })
    ).json()
  ) as { id: string };

  await page.reload();
  await page.locator(`nav div[title="${title}"]`).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  const composer = page.locator("textarea");
  await composer.fill("SLOW tick probe first");
  await composer.press("Enter");
  await expect(page.getByText(/first \d+\.\d+s/)).toBeVisible({ timeout: 20_000 });

  await composer.fill("second tick message");
  await composer.press("Enter");
  await expect(page.getByTestId("pending-row")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("tick-queued")).toBeVisible();

  // After the stream ends the queue flushes; the hub echo replaces the local
  // pending row with a real delivered user bubble.
  await expect(page.getByTestId("pending-row")).toBeHidden({ timeout: 60_000 });
  // Both user messages (direct + queued) earn the delivered double-tick.
  await expect(page.getByTestId("message-scroll").getByTestId("tick-delivered")).toHaveCount(2);
  await page.request.delete(`/api/chats/${chat.id}`);
});

test("large json code block collapses after streaming and downloads", async ({ page }) => {
  await login(page);
  const { providers } = (await (await page.request.get("/api/providers")).json()) as {
    providers: { id: string }[];
  };
  const title = `pw-codeblock-${Date.now()}`;
  const chat = (
    await (
      await page.request.post("/api/chats", {
        data: { title, provider_id: providers[0].id, model: "mock-gpt" },
      })
    ).json()
  ) as { id: string };

  await page.reload();
  await page.locator(`nav div[title="${title}"]`).click();
  await expect(page.getByRole("heading", { name: title })).toBeVisible();
  const scroller = page.getByTestId("message-scroll");
  const composer = page.locator("textarea");
  await composer.fill("give me a JSONBLOCK config please");
  await composer.press("Enter");
  await expect(scroller.getByText("Done.")).toBeVisible({ timeout: 60_000 });

  // Collapsed by default for long structured blocks.
  const collapse = scroller.getByTestId("code-collapse");
  await expect(collapse).toBeVisible();
  await expect(collapse).toHaveAttribute("aria-expanded", "false");
  await expect(scroller.locator("pre")).toHaveCount(0);

  await collapse.click();
  await expect(collapse).toHaveAttribute("aria-expanded", "true");
  await expect(scroller.locator("pre")).toContainText("item-20", { timeout: 5_000 });

  const download = scroller.page().waitForEvent("download");
  await scroller.getByTestId("code-download").click();
  expect((await download).suggestedFilename()).toBe("halotalk-json.json");
  await page.request.delete(`/api/chats/${chat.id}`);
});
