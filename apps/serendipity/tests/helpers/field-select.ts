import { expect, type Locator } from '@playwright/test';

export async function selectField(trigger: Locator, value: string) {
  await expect(trigger).toHaveAttribute('data-slot', 'select-trigger');
  await trigger.click();
  const option = trigger.page().getByRole('option').and(
    trigger.page().locator(`[data-option-value=${JSON.stringify(value)}]`),
  );
  await expect(option).toBeVisible();
  await option.click();
  await expect(trigger).toHaveAttribute('data-value', value);
}
