import { test, expect } from '../fixtures';

test.describe('Settings page', () => {
  test('navigates from sidebar', async ({ serverPage: page }) => {
    await page.goto('/');

    await page.locator('.sidebar-footer a', { hasText: 'Settings' }).click();
    await expect(page).toHaveURL(/\/settings/);
    await expect(page.locator('h2', { hasText: 'Settings' })).toBeVisible();
    await expect(page.locator('h3', { hasText: 'Bookmarklet' })).toBeVisible();
    await expect(page.locator('h3', { hasText: 'Import' })).toBeVisible();
    await expect(page.locator('h3', { hasText: 'Export' })).toBeVisible();
  });

  test('bookmarklet link has javascript href', async ({ serverPage: page }) => {
    await page.goto('/settings');

    const link = page.locator('.bookmarklet-link');
    await expect(link).toBeVisible();
    await expect(link).toHaveText('+ MyBriefcase');
    const href = await link.getAttribute('href');
    expect(href).toContain('javascript:');
    expect(href).toContain('/bookmarks/new');
    expect(href).toContain('encodeURIComponent');
  });

  test('export link points to /export', async ({ serverPage: page }) => {
    await page.goto('/settings');

    const exportLink = page.locator('a[href="/export"]');
    await expect(exportLink).toBeVisible();
    await expect(exportLink).toHaveAttribute('download', 'bookmarks.html');
  });
});

test.describe('Add bookmark page (bookmarklet flow)', () => {
  test('renders with empty fields', async ({ serverPage: page }) => {
    await page.goto('/bookmarks/new');

    await expect(page.locator('h2', { hasText: 'Add Bookmark' })).toBeVisible();
    await expect(page.locator('input[name="title"]')).toHaveValue('');
    await expect(page.locator('input[name="url"]')).toHaveValue('');
    await expect(page.locator('select[name="folder_id"]')).toBeVisible();
  });

  test('pre-fills from query params', async ({ serverPage: page }) => {
    await page.goto('/bookmarks/new?url=https%3A%2F%2Fexample.com%2Fpage&title=My+Page+Title');

    await expect(page.locator('input[name="title"]')).toHaveValue('My Page Title');
    await expect(page.locator('input[name="url"]')).toHaveValue('https://example.com/page');
  });

  test('submits and redirects to folder', async ({ serverPage: page }) => {
    await page.goto('/bookmarks/new?url=https%3A%2F%2Ftest.com&title=Test+Bookmark');

    await page.locator('button[type="submit"]').click();

    await expect(page.locator('.list-item .item-name', { hasText: 'Test Bookmark' })).toBeVisible();
    await expect(page.locator('.list-item .item-url', { hasText: 'test.com' })).toBeVisible();
  });
});
