import { expect, type Locator } from '@playwright/test';

export async function selectField(trigger: Locator, value: string) {
  const dropdown = (await trigger.getAttribute('data-slot')) === 'dropdown-menu-trigger';
  await expect(trigger).toHaveAttribute(
    'data-slot',
    dropdown ? 'dropdown-menu-trigger' : 'select-trigger',
  );
  await trigger.click();
  const option = trigger
    .page()
    .getByRole(dropdown ? 'menuitemradio' : 'option')
    .and(trigger.page().locator(`[data-option-value=${JSON.stringify(value)}]`));
  await expect(option).toBeVisible();
  await option.click();
  await expect(trigger).toHaveAttribute('data-value', value);
}
