import { test, expect } from "@playwright/test";

/** Issue #1165: route guard and shared navigation shell. */
test("unauthenticated visitors are redirected to login", async ({ page }) => {
  await page.goto("/payments");
  await expect(page).toHaveURL(/\/login/);
  await expect(page.getByRole("heading", { name: /sign in/i })).toBeVisible();
});

test("navigation shell highlights the active route", async ({ page }) => {
  await page.goto("/login");
  await page.evaluate(() => localStorage.setItem("fluxapay_jwt", "test-token"));
  await page.goto("/payments");
  const link = page.getByRole("link", { name: /payments/i });
  await expect(link).toHaveAttribute("aria-current", "page");
});
