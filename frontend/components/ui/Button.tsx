'use client'

import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from 'react'
import { Spinner } from './Spinner'

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger' | 'buy' | 'sell'
type Size = 'sm' | 'md' | 'lg'

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant
  size?: Size
  loading?: boolean
  icon?: ReactNode
  iconPosition?: 'left' | 'right'
}

const variantStyles: Record<Variant, string> = {
  primary: [
    'bg-ochre text-[#11121a]',
    'hover:bg-[#a79ef2] active:bg-[#8179d9]',
    'focus-visible:ring-2 focus-visible:ring-ochre/40 focus-visible:ring-offset-2 focus-visible:ring-offset-parchment',
    'disabled:opacity-40',
  ].join(' '),

  buy: [
    'bg-sage text-parchment',
    'hover:bg-[#27b47d] active:bg-[#168f63]',
    'focus-visible:ring-2 focus-visible:ring-sage/40 focus-visible:ring-offset-2 focus-visible:ring-offset-parchment',
    'disabled:opacity-40',
  ].join(' '),

  sell: [
    'bg-terra text-parchment',
    'hover:bg-[#dc5f52] active:bg-[#bc4e44]',
    'focus-visible:ring-2 focus-visible:ring-terra/40 focus-visible:ring-offset-2 focus-visible:ring-offset-parchment',
    'disabled:opacity-40',
  ].join(' '),

  secondary: [
    'bg-transparent text-ochre border-[1.5px] border-ochre',
    'hover:bg-ochre/8 hover:border-ochre active:bg-ochre/12',
    'focus-visible:ring-2 focus-visible:ring-ochre/30 focus-visible:ring-offset-2 focus-visible:ring-offset-parchment',
    'disabled:opacity-40',
  ].join(' '),

  ghost: [
    'bg-transparent text-brown',
    'hover:bg-[rgba(255,255,255,0.04)] hover:text-ink active:bg-[rgba(255,255,255,0.07)]',
    'focus-visible:ring-2 focus-visible:ring-brown/30 focus-visible:ring-offset-2 focus-visible:ring-offset-parchment',
    'disabled:opacity-40',
  ].join(' '),

  danger: [
    'bg-terra text-parchment',
    'hover:bg-[#dc5f52] active:bg-[#bc4e44]',
    'focus-visible:ring-2 focus-visible:ring-terra/40 focus-visible:ring-offset-2 focus-visible:ring-offset-parchment',
    'disabled:opacity-40',
  ].join(' '),
}

const sizeStyles: Record<Size, string> = {
  sm: 'h-8 px-3 text-[0.75rem] gap-1.5',
  md: 'h-10 px-4 text-[0.8rem] gap-2',
  lg: 'h-12 px-6 text-[0.85rem] gap-2.5',
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(
  function Button(
    {
      variant = 'primary',
      size = 'md',
      loading = false,
      icon,
      iconPosition = 'left',
      disabled,
      className = '',
      children,
      ...props
    },
    ref,
  ) {
    const isDisabled = disabled || loading

    return (
      <button
        ref={ref}
        disabled={isDisabled}
        className={[
          'inline-flex items-center justify-center font-medium font-sans',
          'uppercase tracking-[0.08em]',
          'transition-all duration-150 outline-none select-none',
          'cursor-pointer disabled:cursor-not-allowed',
          variantStyles[variant],
          sizeStyles[size],
          className,
        ].join(' ')}
        {...props}
      >
        {loading ? (
          <>
            <Spinner size={size === 'lg' ? 'md' : 'sm'} className="opacity-80" />
            {children && <span className="opacity-70">{children}</span>}
          </>
        ) : (
          <>
            {icon && iconPosition === 'left' && <span className="shrink-0">{icon}</span>}
            {children}
            {icon && iconPosition === 'right' && <span className="shrink-0">{icon}</span>}
          </>
        )}
      </button>
    )
  },
)
