'use client';

import type { ComponentProps, ReactNode } from 'react';
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from './select';
import { cn } from '@/lib/utils';

export type FieldOption = { value: string; label: ReactNode; disabled?: boolean };
type FieldSelectProps = Omit<
  ComponentProps<typeof SelectTrigger>,
  'value' | 'onChange' | 'children'
> & {
  value: string | number;
  onValueChange: (value: string) => void;
  options: FieldOption[];
  name?: string;
  placeholder?: string;
};
// Radix reserves an empty value for its placeholder. Keep empty filter values
// in the caller's domain, without changing their URL or form semantics.
const emptyValue = '__illyaverse_empty_option__';

export function FieldSelect({
  value,
  onValueChange,
  options,
  name,
  placeholder,
  className,
  disabled,
  ...props
}: FieldSelectProps) {
  const selected = String(value);
  return (
    <Select
      name={name}
      disabled={disabled}
      value={selected || emptyValue}
      onValueChange={(next) => onValueChange(next === emptyValue ? '' : next)}
    >
      <SelectTrigger
        {...props}
        name={name}
        data-field-name={name}
        disabled={disabled}
        data-value={selected}
        className={cn('field-select', className)}
      >
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent
        position="popper"
        align="start"
        data-material="floating"
        className="field-select-menu"
      >
        {options.map((option) => (
          <SelectItem
            key={option.value}
            value={option.value || emptyValue}
            disabled={option.disabled}
            data-option-value={option.value}
          >
            {option.label}
          </SelectItem>
        ))}
      </SelectContent>
    </Select>
  );
}
