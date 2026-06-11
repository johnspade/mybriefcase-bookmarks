import { test, expect } from '../fixtures';

test.describe('View mode persistence', () => {
  test('defaults to list view', async ({ serverPage: page }) => {
    await page.goto('/');
    await expect(page.locator('.items-list')).toBeVisible();
    await expect(page.locator('.items-grid')).not.toBeVisible();
  });

  test('persists grid view across reloads', async ({ serverPage: page }) => {
    await page.goto('/');

    // Switch to grid view
    await page.locator('button[title="Grid view"]').click();
    await expect(page.locator('.items-grid')).toBeVisible();
    await expect(page.locator('.items-list')).not.toBeVisible();

    // Reload and verify grid view persists
    await page.reload();
    await expect(page.locator('.items-grid')).toBeVisible();
    await expect(page.locator('.items-list')).not.toBeVisible();
  });

  test('persists list view after switching back', async ({ serverPage: page }) => {
    await page.goto('/');

    // Switch to grid then back to list
    await page.locator('button[title="Grid view"]').click();
    await page.locator('button[title="List view"]').click();
    await expect(page.locator('.items-list')).toBeVisible();

    // Reload and verify list view persists
    await page.reload();
    await expect(page.locator('.items-list')).toBeVisible();
    await expect(page.locator('.items-grid')).not.toBeVisible();
  });
});
