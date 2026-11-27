import { test, expect, type Page, type Route } from '@playwright/test';

/**
 * E2E tests for the payment link checkout flow.
 *
 * The backend is fully mocked via Playwright's `page.route` interception so
 * these tests never hit a real API. Each scenario seeds the payment link
 * metadata endpoint and the payment submission endpoint with deterministic
 * fixtures.
 */

type PaymentLinkFixture = {
  id: string;
  amount: string;
  currency: string;
  status: 'active' | 'expired' | 'partially_funded' | 'completed';
  allow_partial: boolean;
  funded_amount?: string;
};

const ACTIVE_LINK: PaymentLinkFixture = {
  id: 'plink_active_001',
  amount: '100.00',
  currency: 'USDC',
  status: 'active',
  allow_partial: false,
};

const EXPIRED_LINK: PaymentLinkFixture = {
  id: 'plink_expired_001',
  amount: '100.00',
  currency: 'USDC',
  status: 'expired',
  allow_partial: false,
};

const PARTIAL_LINK: PaymentLinkFixture = {
  id: 'plink_partial_001',
  amount: '100.00',
  currency: 'USDC',
  status: 'active',
  allow_partial: true,
  funded_amount: '0.00',
};

/**
 * Installs route handlers that emulate the payment link backend.
 * `links` maps a link id to its fixture; unknown ids return 404.
 */
async function mockBackend(page: Page, links: Record<string, PaymentLinkFixture>) {
  await page.route('**/api/payment-links/**', async (route: Route) => {
    const url = new URL(route.request().url());
    const segments = url.pathname.split('/').filter(Boolean);
    const linkId = segments[segments.length - 1];
    const link = links[linkId];

    if (!link) {
      await route.fulfill({
        status: 404,
        contentType: 'application/json',
        body: JSON.stringify({ error: 'Payment link not found' }),
      });
      return;
    }

    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify(link),
    });
  });

  await page.route('**/api/payments', async (route: Route) => {
    const payload = route.request().postDataJSON() as { amount?: string };
    const amount = payload?.amount ?? '0.00';
    const isPartial = Number(amount) < 100;

    await route.fulfill({
      status: 200,
      contentType: 'application/json',
      body: JSON.stringify({
        id: 'pay_mock_001',
        status: isPartial ? 'partially_funded' : 'completed',
        amount,
        currency: 'USDC',
      }),
    });
  });
}

async function submitUsdcPayment(page: Page, amount: string) {
  await page.getByLabel(/amount/i).fill(amount);
  await page.getByRole('button', { name: /pay/i }).click();
}

test.describe('payment link checkout flow', () => {
  test('happy path: pays correct USDC amount and sees confirmation', async ({ page }) => {
    await mockBackend(page, { [ACTIVE_LINK.id]: ACTIVE_LINK });

    await page.goto(`/pay/${ACTIVE_LINK.id}`);

    await expect(page.getByTestId('checkout-widget')).toBeVisible();
    await expect(page.getByText('100.00 USDC')).toBeVisible();

    await submitUsdcPayment(page, '100.00');

    await expect(page.getByTestId('payment-confirmation')).toBeVisible();
    await expect(page.getByText(/payment successful/i)).toBeVisible();
  });

  test('expired link: shows expired message', async ({ page }) => {
    await mockBackend(page, { [EXPIRED_LINK.id]: EXPIRED_LINK });

    await page.goto(`/pay/${EXPIRED_LINK.id}`);

    await expect(page.getByText(/this link has expired/i)).toBeVisible();
    await expect(page.getByTestId('checkout-widget')).toHaveCount(0);
  });

  test('invalid link: shows 404 page', async ({ page }) => {
    await mockBackend(page, {});

    await page.goto('/pay/plink_does_not_exist');

    await expect(page.getByText(/404/i)).toBeVisible();
    await expect(page.getByText(/not found/i)).toBeVisible();
  });

  test('partial payment: pays half and sees partially funded status', async ({ page }) => {
    await mockBackend(page, { [PARTIAL_LINK.id]: PARTIAL_LINK });

    await page.goto(`/pay/${PARTIAL_LINK.id}`);

    await expect(page.getByTestId('checkout-widget')).toBeVisible();

    await submitUsdcPayment(page, '50.00');

    await expect(page.getByTestId('payment-confirmation')).toBeVisible();
    await expect(page.getByText(/partially funded/i)).toBeVisible();
  });

  test('mobile viewport: happy path on iPhone 12', async ({ browser }) => {
    const context = await browser.newContext({
      viewport: { width: 390, height: 844 },
      userAgent:
        'Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1',
      isMobile: true,
      hasTouch: true,
    });
    const page = await context.newPage();

    await mockBackend(page, { [ACTIVE_LINK.id]: ACTIVE_LINK });

    await page.goto(`/pay/${ACTIVE_LINK.id}`);

    await expect(page.getByTestId('checkout-widget')).toBeVisible();

    await submitUsdcPayment(page, '100.00');

    await expect(page.getByTestId('payment-confirmation')).toBeVisible();

    await context.close();
  });
});
