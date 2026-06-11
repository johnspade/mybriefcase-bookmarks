import { test, expect } from '../fixtures';

test.describe('Search', () => {
  test('search filters bookmarks by title', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create two bookmarks
    for (const [title, url] of [
      ['Alpha Site', 'https://alpha.example.com'],
      ['Beta Page', 'https://beta.example.com'],
    ]) {
      await page.locator('.fab-btn').click();
      await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
      const modal = page.locator('.modal-overlay').first();
      await expect(modal.locator('input[name="title"]')).toBeFocused();
      await modal.locator('input[name="title"]').fill(title);
      await modal.locator('input[name="url"]').fill(url);
      await modal.locator('button[type="submit"]').click();
      await expect(modal).not.toBeVisible();
    }

    // Verify both exist
    await expect(page.locator('.list-item', { hasText: 'Alpha Site' })).toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'Beta Page' })).toBeVisible();

    // Search for "Alpha" — use pressSequentially to fire keyup events for HTMX trigger
    await page.locator('#searchInput').pressSequentially('Alpha', { delay: 50 });

    // Wait for debounced HTMX request to complete and show filtered results
    await expect(page.locator('.list-item', { hasText: 'Alpha Site' })).toBeVisible({ timeout: 10_000 });
    await expect(page.locator('.list-item', { hasText: 'Beta Page' })).not.toBeVisible();
  });
});
