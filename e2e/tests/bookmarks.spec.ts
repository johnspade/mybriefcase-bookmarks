import { test, expect } from '../fixtures';

test.describe('Bookmarks', () => {
  test('add a bookmark', async ({ serverPage: page }) => {
    await page.goto('/');

    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();

    await modal.locator('input[name="title"]').fill('Example Site');
    await modal.locator('input[name="url"]').fill('https://example.com');
    await modal.locator('button[type="submit"]').click();

    await expect(modal).not.toBeVisible();
    await expect(page.locator('.list-item .item-name', { hasText: 'Example Site' })).toBeVisible();
    await expect(page.locator('.list-item .item-url', { hasText: 'example.com' })).toBeVisible();
  });

  test('view bookmark details', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark first
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('Detail Test');
    await modal.locator('input[name="url"]').fill('https://detail.example.com');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();

    // Click the bookmark to see details
    await page.locator('.list-item', { hasText: 'Detail Test' }).click();

    await expect(page.locator('#detail-body .detail-title')).toHaveText('Detail Test');
    await expect(page.locator('#detail-body .detail-url')).toHaveText('https://detail.example.com');
  });

  test('edit a bookmark', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const addModal = page.locator('.modal-overlay').first();
    await expect(addModal.locator('input[name="title"]')).toBeFocused();
    await addModal.locator('input[name="title"]').fill('Edit Me');
    await addModal.locator('input[name="url"]').fill('https://original.com');
    await addModal.locator('button[type="submit"]').click();
    await expect(addModal).not.toBeVisible();

    // Open details
    await page.locator('.list-item', { hasText: 'Edit Me' }).click();
    await expect(page.locator('#detail-body .detail-title')).toHaveText('Edit Me', { timeout: 10_000 });

    // Click Edit to open the edit modal
    await page.locator('#detail-body button', { hasText: 'Edit' }).click();
    const editModal = page.locator('#edit-modal-body');
    await expect(editModal).toBeVisible({ timeout: 10_000 });

    // Edit fields in the modal
    await editModal.locator('input[name="title"]').fill('Edited Title');
    await editModal.locator('input[name="url"]').fill('https://edited.com');
    await editModal.locator('textarea[name="notes"]').fill('Some notes');

    // Save
    const saveResponse = page.waitForResponse(resp => resp.url().includes('/edit'));
    await editModal.locator('button[type="submit"]').click();
    await saveResponse;

    // Verify detail updated
    await expect(page.locator('#detail-body .detail-title')).toHaveText('Edited Title');
    await expect(page.locator('#detail-body .detail-url')).toHaveText('https://edited.com');
  });

  test('delete a bookmark', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await expect(modal.locator('input[name="title"]')).toBeFocused();
    await modal.locator('input[name="title"]').fill('Delete Me');
    await modal.locator('input[name="url"]').fill('https://deleteme.com');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();
    await expect(page.locator('.list-item', { hasText: 'Delete Me' })).toBeVisible();

    // Accept the confirm dialog
    page.on('dialog', dialog => dialog.accept());

    // Hover to reveal three-dot menu, then click delete
    const deleteItem = page.locator('.list-item', { hasText: 'Delete Me' });
    await deleteItem.hover();
    await deleteItem.locator('.item-menu-trigger').click();
    await deleteItem.locator('.item-menu-option', { hasText: 'Delete' }).click();

    await expect(page.locator('.list-item', { hasText: 'Delete Me' })).not.toBeVisible();
  });

  test('move bookmark via edit form folder picker', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create Folder A
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Folder' }).click();
    const folderModal1 = page.locator('.modal-overlay').first();
    await folderModal1.locator('input[name="title"]').fill('Folder A');
    await folderModal1.locator('button[type="submit"]').click();
    await expect(folderModal1).not.toBeVisible();

    // Create Folder B
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
    await bmModal.locator('input[name="title"]').fill('Move Via Edit');
    await bmModal.locator('input[name="url"]').fill('https://move-edit.example.com');
    await bmModal.locator('button[type="submit"]').click();
    await expect(bmModal).not.toBeVisible();

    // Open bookmark detail
    await page.locator('.list-item', { hasText: 'Move Via Edit' }).click();
    await expect(page.locator('#detail-body .detail-title')).toHaveText('Move Via Edit', { timeout: 10_000 });

    // Click Edit to open the edit modal
    await page.locator('#detail-body button', { hasText: 'Edit' }).click();
    const editModal = page.locator('#edit-modal-body');
    await expect(editModal).toBeVisible({ timeout: 10_000 });

    // Change folder to Folder B via radio button
    await editModal.locator('.move-list-item', { hasText: 'Folder B' }).click();

    // Save
    const saveResponse = page.waitForResponse(resp => resp.url().includes('/edit'));
    await editModal.locator('button[type="submit"]').click();
    await saveResponse;

    // Bookmark should no longer be in Folder A - we should now be viewing Folder B
    await expect(page.locator('.breadcrumb')).toContainText('Folder B');
    await expect(page.locator('.list-item', { hasText: 'Move Via Edit' })).toBeVisible();
  });
});
