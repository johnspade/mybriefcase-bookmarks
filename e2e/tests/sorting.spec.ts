import { test, expect } from '../fixtures';

test.describe('Sorting bookmarks', () => {
  async function createBookmark(page: any, title: string, url: string) {
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal).toBeVisible();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill(title);
    await modal.locator('input[name="url"]').fill(url);
    await modal.locator('button[type="submit"]').click();
    await expect(page.locator('.list-item', { hasText: title })).toBeVisible({ timeout: 10000 });
  }

  function bookmarkItems(page: any) {
    return page.locator('.list-item[hx-get*="/bookmarks/"] .item-name');
  }

  test('opens settings popover and shows sort options', async ({ serverPage: page }) => {
    await page.goto('/');

    await page.locator('button[title="View settings"]').click();
    const popover = page.locator('.settings-popover');
    await expect(popover).toBeVisible();
    await expect(popover.locator('.popover-label', { hasText: 'View' })).toBeVisible();
    await expect(popover.locator('.popover-label', { hasText: 'Sort by' })).toBeVisible();
    await expect(popover.locator('.sort-option', { hasText: 'Name A→Z' })).toBeVisible();
    await expect(popover.locator('.sort-option', { hasText: 'Name Z→A' })).toBeVisible();
    await expect(popover.locator('.sort-option', { hasText: 'Newest first' })).toBeVisible();
    await expect(popover.locator('.sort-option', { hasText: 'Oldest first' })).toBeVisible();
  });

  test('closes popover when clicking outside', async ({ serverPage: page }) => {
    await page.goto('/');

    await page.locator('button[title="View settings"]').click();
    await expect(page.locator('.settings-popover')).toBeVisible();

    await page.locator('.breadcrumb').click();
    await expect(page.locator('.settings-popover')).not.toBeVisible();
  });

  test('sorts bookmarks by name descending', async ({ serverPage: page }) => {
    await page.goto('/');
    await createBookmark(page, 'Alpha', 'https://alpha.example.com');
    await createBookmark(page, 'Charlie', 'https://charlie.example.com');
    await createBookmark(page, 'Bravo', 'https://bravo.example.com');

    // Default should be name ascending
    const items = bookmarkItems(page);
    await expect(items.nth(0)).toHaveText('Alpha');
    await expect(items.nth(1)).toHaveText('Bravo');
    await expect(items.nth(2)).toHaveText('Charlie');

    // Sort by name descending
    await page.locator('button[title="View settings"]').click();
    await page.locator('.sort-option', { hasText: 'Name Z→A' }).click();

    // Wait for HTMX re-fetch to complete
    const itemsAfter = bookmarkItems(page);
    await expect(itemsAfter.nth(0)).toHaveText('Charlie');
    await expect(itemsAfter.nth(1)).toHaveText('Bravo');
    await expect(itemsAfter.nth(2)).toHaveText('Alpha');
  });

  test('sorts bookmarks by date (newest first)', async ({ serverPage: page }) => {
    await page.goto('/');
    await createBookmark(page, 'First', 'https://first.example.com');
    await createBookmark(page, 'Second', 'https://second.example.com');
    await createBookmark(page, 'Third', 'https://third.example.com');

    // Sort by newest first
    await page.locator('button[title="View settings"]').click();
    await page.locator('.sort-option', { hasText: 'Newest first' }).click();

    const items = bookmarkItems(page);
    await expect(items.nth(0)).toHaveText('Third');
    await expect(items.nth(1)).toHaveText('Second');
    await expect(items.nth(2)).toHaveText('First');
  });

  test('persists sort preference in localStorage', async ({ serverPage: page }) => {
    await page.goto('/');
    await createBookmark(page, 'Zebra', 'https://zebra.example.com');
    await createBookmark(page, 'Apple', 'https://apple.example.com');

    // Switch to name descending
    await page.locator('button[title="View settings"]').click();
    await page.locator('.sort-option', { hasText: 'Name Z→A' }).click();

    const items = bookmarkItems(page);
    await expect(items.nth(0)).toHaveText('Zebra');

    const sortValue = await page.evaluate(() => localStorage.getItem('sort-order'));
    expect(sortValue).toBe('name_desc');
  });

  test('view mode toggle works inside popover', async ({ serverPage: page }) => {
    await page.goto('/');
    await createBookmark(page, 'Test Bookmark', 'https://test.example.com');

    await expect(page.locator('.items-list')).toBeVisible();
    await expect(page.locator('.items-grid')).not.toBeVisible();

    // Switch to grid via popover
    await page.locator('button[title="View settings"]').click();
    await page.locator('.popover-btn', { hasText: 'Grid' }).click();

    await expect(page.locator('.items-grid')).toBeVisible();
    await expect(page.locator('.items-list')).not.toBeVisible();
  });
});
