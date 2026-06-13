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
    expect(href).toContain('/?url=');
    expect(href).toContain('encodeURIComponent');
  });

  test('export link points to /export', async ({ serverPage: page }) => {
    await page.goto('/settings');

    const exportLink = page.locator('a[href="/export"]');
    await expect(exportLink).toBeVisible();
    await expect(exportLink).toHaveAttribute('download', 'bookmarks.html');
  });
});

test.describe('Settings page layout', () => {
  test('renders without sidebars or FAB', async ({ serverPage: page }) => {
    await page.goto('/settings');

    await expect(page.locator('.settings-layout')).toBeVisible();
    await expect(page.locator('.sidebar')).not.toBeAttached();
    await expect(page.locator('.fab-btn')).not.toBeAttached();
    await expect(page.locator('.detail-panel')).not.toBeAttached();
  });

  test('has back button linking to home', async ({ serverPage: page }) => {
    await page.goto('/settings');

    const backLink = page.locator('.toolbar-btn[href="/"]');
    await expect(backLink).toBeVisible();
  });
});

test.describe('Import from settings', () => {
  test('imports bookmarks to root folder and redirects home', async ({ serverPage: page }) => {
    await page.goto('/settings');

    const fileContent = `<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><A HREF="https://imported-example.com">Imported Example</A>
<DT><A HREF="https://imported-rust.org">Imported Rust</A>
</DL>`;

    const fileInput = page.locator('#import-file');
    await fileInput.setInputFiles({
      name: 'bookmarks.html',
      mimeType: 'text/html',
      buffer: Buffer.from(fileContent),
    });

    await page.locator('button[type="submit"]', { hasText: 'Import' }).click();

    await page.waitForURL('/');
    await expect(page.locator('.list-item .item-name', { hasText: 'Imported Example' })).toBeVisible();
    await expect(page.locator('.list-item .item-name', { hasText: 'Imported Rust' })).toBeVisible();
  });

  test('imports bookmarks to new folder and redirects home', async ({ serverPage: page }) => {
    await page.goto('/settings');

    await page.locator('#import-target').selectOption('new');

    const fileContent = `<!DOCTYPE NETSCAPE-Bookmark-file-1>
<DL><p>
<DT><A HREF="https://new-folder-test.com">New Folder Bookmark</A>
</DL>`;

    const fileInput = page.locator('#import-file');
    await fileInput.setInputFiles({
      name: 'bookmarks.html',
      mimeType: 'text/html',
      buffer: Buffer.from(fileContent),
    });

    await page.locator('button[type="submit"]', { hasText: 'Import' }).click();

    await page.waitForURL('/');
    const importedFolder = page.locator('.list-item .item-name', { hasText: /Imported \d{4}-\d{2}-\d{2}/ });
    await expect(importedFolder).toBeVisible();
    await importedFolder.click();
    await expect(page.locator('.list-item .item-name', { hasText: 'New Folder Bookmark' })).toBeVisible();
  });
});

test.describe('Add bookmark modal', () => {
  test('FAB opens modal with empty fields and folder select', async ({ serverPage: page }) => {
    await page.goto('/');

    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();

    await expect(page.locator('.modal h2', { hasText: 'Add Bookmark' })).toBeVisible();
    await expect(page.locator('.modal input[name="title"]')).toHaveValue('');
    await expect(page.locator('.modal input[name="url"]')).toHaveValue('');
    await expect(page.locator('.modal select[name="folder_id"]')).toBeVisible();
  });

  test('bookmarklet query params pre-fill modal', async ({ serverPage: page }) => {
    await page.goto('/?url=https%3A%2F%2Fexample.com%2Fpage&title=My+Page+Title');

    await expect(page.locator('.modal h2', { hasText: 'Add Bookmark' })).toBeVisible();
    await expect(page.locator('.modal input[name="title"]')).toHaveValue('My Page Title');
    await expect(page.locator('.modal input[name="url"]')).toHaveValue('https://example.com/page');
  });

  test('submits bookmark from pre-filled modal', async ({ serverPage: page }) => {
    await page.goto('/?url=https%3A%2F%2Ftest.com&title=Test+Bookmark');

    await page.locator('.modal button[type="submit"]').click();

    await expect(page.locator('.list-item .item-name', { hasText: 'Test Bookmark' })).toBeVisible();
    await expect(page.locator('.list-item .item-url', { hasText: 'test.com' })).toBeVisible();
  });
});

