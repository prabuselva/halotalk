import { APIRequestContext, expect, Page, test } from "@playwright/test";

// Auth UX: 2-step signup wizard with a recovery question, per-field inline
// errors, and login failure feedback. Requires open sign-up on the instance;
// the new account name is unique per run.
const BASE = process.env.E2E_BASE ?? "http://127.0.0.1:8420";
const BOB = process.env.E2E_USER ?? "bob";
const NEW_USER = `pw${Date.now().toString(36)}`;
const NEW_PASS = "playwright-pass-1";
const RECOVERY_ANSWER = "Snowball";

async function openSignup(page: Page) {
  await page.goto("/");
  await page.getByRole("button", { name: "Create an account" }).click();
}

async function fillStep1(page: Page, username: string, password: string) {
  await page.locator('input[autocomplete="username"]').fill(username);
  await page.locator('input[autocomplete="new-password"]').fill(password);
  await page.getByRole("button", { name: "Continue" }).click();
}

// The e2e environment configures open sign-up permanently; an admin-only
// toggle can't be exercised from these specs (E2E_USER is not an admin).
test.beforeAll(async ({ playwright }) => {
  const req: APIRequestContext = await playwright.request.newContext({ baseURL: BASE });
  const status = await (await req.get("/api/auth/status")).json();
  await req.dispose();
  if (status.needs_setup === true || status.open_signup !== true) {
    throw new Error("auth e2e requires the instance to exist with open sign-up enabled");
  }
});

test("version shows on the auth screen and sidebar", async ({ page }) => {
  const health = await (await page.request.get("/api/health")).json();
  expect(health.version).toMatch(/^\d+\.\d+\.\d+/);

  // auth screen (not logged in)
  await page.goto("/");
  await expect(page.getByTestId("app-version")).toHaveText(`v${health.version}`);

  // sidebar footer after login
  await page.locator('input[autocomplete="username"]').fill(BOB);
  await page.locator('input[autocomplete="current-password"]').fill("bobpassword");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("button", { name: "New chat" })).toBeVisible();
  await expect(page.getByTestId("app-version")).toHaveText(`v${health.version}`);
});

test("signup wizard collects a recovery question and auto-logs-in", async ({ page }) => {
  await openSignup(page);
  await fillStep1(page, NEW_USER, NEW_PASS);
  await expect(page.getByText("Step 2 of 2")).toBeVisible();

  // answer + confirm (both autocomplete=off inputs)
  const answers = page.locator('input[autocomplete="off"]');
  await answers.nth(0).fill(RECOVERY_ANSWER);
  await answers.nth(1).fill("nope");
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page.getByText("answers do not match")).toBeVisible();

  await answers.nth(1).fill(RECOVERY_ANSWER);
  await page.getByRole("button", { name: "Create account" }).click();
  await expect(page.getByRole("button", { name: "New chat" })).toBeVisible();
});

test("empty signup fields show per-field errors without navigation", async ({ page }) => {
  await openSignup(page);
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("username is required")).toBeVisible();
  await expect(page.getByText("password is required")).toBeVisible();

  await page.locator('input[autocomplete="username"]').fill("ab");
  await page
    .locator('input[autocomplete="new-password"]')
    .fill("short");
  await page.getByRole("button", { name: "Continue" }).click();
  await expect(page.getByText("3-32 characters")).toBeVisible();
  await expect(page.getByText("password must be at least 8 characters")).toBeVisible();
  await expect(page.getByText("Step 2 of 2")).not.toBeVisible();
});

test("taken username bounces back to step 1 with an inline error", async ({ page }) => {
  await openSignup(page);
  await fillStep1(page, BOB, NEW_PASS);
  await expect(page.getByText("Step 2 of 2")).toBeVisible();
  const answers = page.locator('input[autocomplete="off"]');
  await answers.nth(0).fill(RECOVERY_ANSWER);
  await answers.nth(1).fill(RECOVERY_ANSWER);
  await page.getByRole("button", { name: "Create account" }).click();

  await expect(page.getByText("username already taken")).toBeVisible();
  await expect(page.getByText("Step 2 of 2")).not.toBeVisible();
});

test("login shows an inline alert on wrong password and succeeds after", async ({ page }) => {
  await page.goto("/");
  await page.locator('input[autocomplete="username"]').fill(NEW_USER);
  await page.locator('input[autocomplete="current-password"]').fill("completely-wrong");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("wrong username or password")).toBeVisible();

  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByText("wrong username or password")).toBeVisible();

  await page.locator('input[autocomplete="current-password"]').fill(NEW_PASS);
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.getByRole("button", { name: "New chat" })).toBeVisible();
});
