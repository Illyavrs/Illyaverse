import type { ComponentProps } from 'react';

export function Button({ variant = 'primary', className = '', ...props }: ComponentProps<'button'> & {
  variant?: 'primary' | 'outline' | 'quiet';
}) {
  return <button data-slot="button" className={`button button-${variant} ${className}`} {...props} />;
}
