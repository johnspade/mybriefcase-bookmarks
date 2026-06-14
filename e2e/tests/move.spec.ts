import { test, expect } from '../fixtures';

test.describe('Move items', () => {
  test('move a bookmark to another folder', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create folder A
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const folderModal = page.locator('.modal-overlay').first();
    await folderModal.locator('input[name="title"]').fill('Folder A');
    await folderModal.locator('button[type="submit"]').click();
    await expect(folderModal).not.toBeVisible();

    // Create folder B
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const folderModal2 = page.locator('.modal-overlay').first();
    await folderModal2.locator('input[name="title"]').fill('Folder B');
    await folderModal2.locator('button[type="submit"]').click();
    await expect(folderModal2).not.toBeVisible();

    // Navigate into Folder A
    await page.locator('.list-item', { hasText: 'Folder A' }).click();
    await expect(page.locator('.breadcrumb')).toContainText('Folder A');

    // Create a bookmark inside Folder A
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const bmModal = page.locator('.modal-overlay').first();
    await expect(bmModal.locator('input[name="title"]')).toBeFocused();
    await bmModal.locator('input[name="title"]').fill('Moveable Bookmark');
    await bmModal.locator('input[name="url"]').fill('https://moveable.example.com');
    await bmModal.locator('button[type="submit"]').click();
    await expect(bmModal).not.toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'Moveable Bookmark' })).toBeVisible();

    // Open three-dot menu on the bookmark and click "Move to..."
    const bmItem = page.locator('.list-item', { hasText: 'Moveable Bookmark' });
    await bmItem.hover();
    await bmItem.locator('.item-menu-trigger').click();
    await bmItem.locator('.item-menu-option', { hasText: 'Move to' }).click();

    // Move modal should appear
    const moveModal = page.locator('.modal-overlay:visible');
    await expect(moveModal).toBeVisible();
    await expect(moveModal).toContainText('Move to');

    // Select Folder B as destination
    await moveModal.locator('.move-list-item', { hasText: 'Folder B' }).locator('input[type="radio"]').check();
    await moveModal.locator('button[type="submit"]').click();

    // Modal should close and bookmark should be gone from current folder
    await expect(moveModal).not.toBeVisible();

    // Navigate to Folder B and verify the bookmark is there
    await page.locator('.breadcrumb span', { hasText: 'Bookmarks' }).first().click();
    await page.locator('.list-item', { hasText: 'Folder B' }).click();
    await expect(page.locator('.breadcrumb')).toContainText('Folder B');
    await expect(page.locator('.list-item', { hasText: 'Moveable Bookmark' })).toBeVisible();
  });

  test('move a folder into another folder', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create folder A
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal1 = page.locator('.modal-overlay').first();
    await modal1.locator('input[name="title"]').fill('Target Folder');
    await modal1.locator('button[type="submit"]').click();
    await expect(modal1).not.toBeVisible();

    // Create folder B
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const modal2 = page.locator('.modal-overlay').first();
    await modal2.locator('input[name="title"]').fill('Move Me');
    await modal2.locator('button[type="submit"]').click();
    await expect(modal2).not.toBeVisible();

    // Both folders should be visible at root
    await expect(page.locator('.list-item', { hasText: 'Target Folder' })).toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'Move Me' })).toBeVisible();

    // Open three-dot menu on "Move Me" folder and click "Move to..."
    const moveItem = page.locator('.list-item', { hasText: 'Move Me' });
    await moveItem.hover();
    await moveItem.locator('.item-menu-trigger').click();
    await moveItem.locator('.item-menu-option', { hasText: 'Move to' }).click();

    // Move modal should appear
    const moveModal = page.locator('.modal-overlay:visible');
    await expect(moveModal).toBeVisible();

    // Select Target Folder as destination
    await moveModal.locator('.move-list-item', { hasText: 'Target Folder' }).locator('input[type="radio"]').check();
    await moveModal.locator('button[type="submit"]').click();

    // Modal should close — after move, page navigates to destination folder
    await expect(moveModal).not.toBeVisible();
    await expect(page.locator('.breadcrumb')).toContainText('Target Folder');
    await expect(page.locator('.list-item', { hasText: 'Move Me' })).toBeVisible();

    // Sidebar should reflect the new structure
    await expect(page.locator('#sidebar-tree')).toContainText('Move Me');
  });
});
