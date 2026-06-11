import { test, expect } from '../fixtures';
import AxeBuilder from '@axe-core/playwright';

test.describe('Accessibility', () => {
  test('home page passes axe WCAG checks', async ({ serverPage: page }) => {
    await page.goto('/');
    const results = await new AxeBuilder({ page })
      .withTags(['wcag2a', 'wcag2aa'])
      .analyze();
    expect(results.violations).toEqual([]);
  });
});
