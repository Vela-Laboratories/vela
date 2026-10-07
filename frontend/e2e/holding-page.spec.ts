import { expect, test } from '@playwright/test'

test('the public root shows the Dippi holding page', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'dippi.fun is coming soon' })).toBeVisible()
  await expect(page.locator('video.coming-soon__video')).toHaveCount(1)
  await expect(page.getByRole('button', { name: 'Connect wallet' })).toHaveCount(0)
})

for (const privateRoute of ['/markets', '/deposit', '/withdraw', '/profile', '/agents']) {
  test(`${privateRoute} redirects to the holding page`, async ({ request }) => {
    const response = await request.get(privateRoute, { maxRedirects: 0 })
    expect(response.status()).toBe(307)
    expect(new URL(response.headers().location, response.url()).pathname).toBe('/')
  })
}
