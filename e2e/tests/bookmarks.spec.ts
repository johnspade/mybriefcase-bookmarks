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
});
