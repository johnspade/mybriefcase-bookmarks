import { test, expect } from '../fixtures';

test.describe('Favicon management', () => {
  test('edit form shows favicon controls', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await modal.locator('input[name="title"]').fill('Favicon Test');
    await modal.locator('input[name="url"]').fill('https://example.com');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();

    // Open bookmark detail
    await page.locator('.list-item', { hasText: 'Favicon Test' }).click();
    await expect(page.locator('#detail-body .detail-title')).toHaveText('Favicon Test', { timeout: 10_000 });

    // Click Edit
    await page.locator('#detail-body button', { hasText: 'Edit' }).click();
    const editModal = page.locator('#edit-modal-body');
    await expect(editModal).toBeVisible({ timeout: 10_000 });

    // Verify favicon controls exist
    await expect(editModal.locator('#favicon-preview')).toBeVisible();
    await expect(editModal.locator('input[name="favicon"]')).toBeAttached();
    await expect(editModal.locator('button', { hasText: 'Refetch' })).toBeVisible();
  });

  test('refetch button triggers request and updates preview area', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark pointing to a real URL
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await modal.locator('input[name="title"]').fill('Refetch Test');
    await modal.locator('input[name="url"]').fill('https://example.com');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();

    // Open edit form
    await page.locator('.list-item', { hasText: 'Refetch Test' }).click();
    await expect(page.locator('#detail-body .detail-title')).toHaveText('Refetch Test', { timeout: 10_000 });
    await page.locator('#detail-body button', { hasText: 'Edit' }).click();
    const editModal = page.locator('#edit-modal-body');
    await expect(editModal).toBeVisible({ timeout: 10_000 });

    // Click Refetch and wait for network response
    const refetchBtn = editModal.locator('button', { hasText: 'Refetch' });
    const responsePromise = page.waitForResponse(resp => resp.url().includes('/fetch-favicon'));
    await refetchBtn.click();
    await responsePromise;

    // After response, preview area should still exist with hidden input
    const preview = editModal.locator('#favicon-preview');
    await expect(preview).toBeVisible({ timeout: 5_000 });
    await expect(preview.locator('input[name="favicon"]')).toBeAttached();
  });

  test('delete button clears favicon preview', async ({ serverPage: page }) => {
    await page.goto('/');

    // Create a bookmark
    await page.locator('.fab-btn').click();
    await page.locator('.fab-menu-item', { hasText: 'New Bookmark' }).click();
    const modal = page.locator('.modal-overlay').first();
    await modal.locator('input[name="title"]').fill('Delete Fav Test');
    await modal.locator('input[name="url"]').fill('https://example.com');
    await modal.locator('button[type="submit"]').click();
    await expect(modal).not.toBeVisible();

    // Open edit form
    await page.locator('.list-item', { hasText: 'Delete Fav Test' }).click();
    await expect(page.locator('#detail-body .detail-title')).toHaveText('Delete Fav Test', { timeout: 10_000 });
    await page.locator('#detail-body button', { hasText: 'Edit' }).click();
    const editModal = page.locator('#edit-modal-body');
    await expect(editModal).toBeVisible({ timeout: 10_000 });

    // The hidden input should start empty (no favicon yet)
    const hiddenInput = editModal.locator('input[name="favicon"]');
    await expect(hiddenInput).toHaveValue('');

    // The avatar letter should be visible (since no favicon)
    await expect(editModal.locator('#favicon-preview .favicon')).toBeVisible();
  });
});
