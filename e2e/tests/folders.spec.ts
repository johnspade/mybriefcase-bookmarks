import { test, expect } from '../fixtures';

test.describe('Folders', () => {
  test('create a folder', async ({ serverPage: page }) => {
    await page.goto('/');

    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();

    await modal.locator('input[name="title"]').fill('Work Projects');
    await modal.locator('button[type="submit"]').click();

    await expect(modal).not.toBeVisible();
    await expect(page.locator('.list-item .item-name', { hasText: 'Work Projects' })).toBeVisible();
    await expect(page.locator('#sidebar-tree')).toContainText('Work Projects');
  });

  test('navigate into a folder', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create folder
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('Sub Folder');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();

    // Click the folder to navigate into it
    await page.locator('.list-item', { hasText: 'Sub Folder' }).click();

    await expect(page.locator('.breadcrumb')).toContainText('Sub Folder');
    await expect(page.locator('#items-panel .detail-empty')).toContainText('This folder is empty');
  });

  test('delete a folder', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create folder
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('To Delete');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'To Delete' })).toBeVisible();

    // Accept confirm dialog
    page.on('dialog', dialog => dialog.accept());

    // Hover to reveal action buttons, then click delete
    await page.locator('.list-item', { hasText: 'To Delete' }).hover();
    await page.locator('.list-item', { hasText: 'To Delete' }).locator('.item-action-btn[title="Delete"]').click();

    await expect(page.locator('.list-item', { hasText: 'To Delete' })).not.toBeVisible();
  });

  test('delete a folder with bookmarks inside', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create folder
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('Cascade Folder');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'Cascade Folder' })).toBeVisible();

    // Navigate into the folder
    await page.locator('.list-item', { hasText: 'Cascade Folder' }).click();
    await expect(page.locator('.breadcrumb')).toContainText('Cascade Folder');

    // Add a bookmark inside
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const bmModal = page.locator('.modal-overlay').first();
    await expect(bmModal.locator('input[name="title"]')).toBeFocused();
    await bmModal.locator('input[name="url"]').fill('https://cascade-test.example.com');
    await bmModal.locator('input[name="title"]').fill('Cascade Bookmark');
    await bmModal.locator('button[type="submit"]').click();
    await expect(bmModal).not.toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'Cascade Bookmark' })).toBeVisible();

    // Navigate back to root via breadcrumb
    await page.locator('.breadcrumb span', { hasText: 'Bookmarks' }).first().click();

    // Accept confirm dialog
    page.on('dialog', dialog => dialog.accept());

    // Delete the folder
    await page.locator('.list-item', { hasText: 'Cascade Folder' }).hover();
    await page.locator('.list-item', { hasText: 'Cascade Folder' }).locator('.item-action-btn[title="Delete"]').click();
    await expect(page.locator('.list-item', { hasText: 'Cascade Folder' })).not.toBeVisible();

    // Verify the bookmark doesn't appear anywhere in the UI
    await expect(page.locator('.list-item', { hasText: 'Cascade Bookmark' })).not.toBeVisible();
  });
});
