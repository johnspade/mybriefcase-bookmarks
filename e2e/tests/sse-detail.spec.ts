import { test, expect } from '../fixtures';

test.describe('SSE detail panel refresh', () => {
  test('detail panel updates when bookmark changes via API', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark via UI
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('SSE Test');
    await modal.locator('input[name="url"]').fill('https://sse-test.example.com');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();

    // Click the bookmark to open detail panel
    await page.locator('.list-item', { hasText: 'SSE Test' }).click();
    await expect(page.locator('#detail-body .detail-title')).toHaveText('SSE Test');

    // Get the bookmark ID from the selected list item
    const bookmarkId = await page.locator('.list-item.selected[data-item-id]').getAttribute('data-item-id');

    // Update the bookmark via the API (triggers SSE refresh)
    const baseURL = page.url().replace(/\/$/, '');
    const resp = await page.request.put(`${baseURL}/api/bookmarks/${bookmarkId}`, {
      data: { title: 'SSE Updated Title' },
    });
    expect(resp.status()).toBe(200);

    // Detail panel should update via SSE
    await expect(page.locator('#detail-body .detail-title')).toHaveText('SSE Updated Title');
  });
});
