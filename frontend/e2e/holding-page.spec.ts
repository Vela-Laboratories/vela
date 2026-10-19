import { expect, test } from '@playwright/test'

test('the public root shows the Vela research archive', async ({ page }) => {
  await page.goto('/')
  await expect(page.getByRole('heading', { name: 'Vela Exchange' })).toBeVisible()
  await expect(page.getByText('Development has concluded.', { exact: false })).toBeVisible()
  await expect(page.getByRole('link', { name: 'View the repository' })).toHaveAttribute('href', 'https://github.com/Vela-Laboratories/exchange')
  await expect(page.getByRole('button', { name: 'Connect wallet' })).toHaveCount(0)
})

for (const privateRoute of ['/markets', '/deposit', '/withdraw', '/profile', '/agents']) {
  test(`${privateRoute} redirects to the holding page`, async ({ request }) => {
    const response = await request.get(privateRoute, { maxRedirects: 0 })
    expect(response.status()).toBe(307)
    expect(new URL(response.headers().location, response.url()).pathname).toBe('/')
  })
}
