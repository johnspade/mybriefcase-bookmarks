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

    // Hover to reveal three-dot menu, then click delete
    const deleteItem = page.locator('.list-item', { hasText: 'To Delete' });
    await deleteItem.hover();
    await deleteItem.locator('.item-menu-trigger').click();
    await deleteItem.locator('.item-menu-option', { hasText: 'Delete' }).click();

    await expect(page.locator('.list-item', { hasText: 'To Delete' })).not.toBeVisible();
  });

  test('rename a folder via three-dot menu', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create folder
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('Before Rename');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.list-item .item-name', { hasText: 'Before Rename' })).toBeVisible();

    // Open three-dot menu and click Rename
    const folderItem = page.locator('.list-item', { hasText: 'Before Rename' });
    await folderItem.hover();
    await folderItem.locator('.item-menu-trigger').click();
    await folderItem.locator('.item-menu-option', { hasText: 'Rename' }).click();

    // Rename modal should appear with pre-filled name
    const renameModal = page.locator('.modal-overlay').first();
    await expect(renameModal).toBeVisible();
    const nameInput = renameModal.locator('input[name="title"]');
    await expect(nameInput).toHaveValue('Before Rename');

    // Clear and type new name
    await nameInput.clear();
    await nameInput.fill('After Rename');
    await renameModal.locator('button[type="submit"]').click();

    // Modal should close and new name should appear
    await expect(renameModal).not.toBeVisible();
    await expect(page.locator('.list-item .item-name', { hasText: 'After Rename' })).toBeVisible();
    await expect(page.locator('.list-item .item-name', { hasText: 'Before Rename' })).not.toBeVisible();

    // Sidebar should also reflect the rename
    await expect(page.locator('#sidebar-tree')).toContainText('After Rename');
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

    // Delete the folder via three-dot menu
    const cascadeItem = page.locator('.list-item', { hasText: 'Cascade Folder' });
    await cascadeItem.hover();
    await cascadeItem.locator('.item-menu-trigger').click();
    await cascadeItem.locator('.item-menu-option', { hasText: 'Delete' }).click();
    await expect(page.locator('.list-item', { hasText: 'Cascade Folder' })).not.toBeVisible();

    // Verify the bookmark doesn't appear anywhere in the UI
    await expect(page.locator('.list-item', { hasText: 'Cascade Bookmark' })).not.toBeVisible();
  });
});
