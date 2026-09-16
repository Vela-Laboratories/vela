import type { ReactNode } from 'react'

type BadgeVariant =
  | 'primary'
  | 'secondary'
  | 'success'
  | 'warning'
  | 'error'
  | 'neutral'

type BadgeSize = 'sm' | 'md'

export interface BadgeProps {
  variant?: BadgeVariant
  size?: BadgeSize
  dot?: boolean
  children: ReactNode
  className?: string
}

const variantStyles: Record<BadgeVariant, string> = {
  primary:   'bg-[rgba(148,136,238,0.12)] text-ochre border border-[rgba(148,136,238,0.25)]',
  secondary: 'bg-[rgba(79,144,217,0.1)] text-fresco border border-[rgba(79,144,217,0.22)]',
  success:   'bg-[rgba(32,201,138,0.1)] text-sage border border-[rgba(32,201,138,0.25)]',
  warning:   'bg-[rgba(212,162,76,0.1)] text-[#d4a24c] border border-[rgba(212,162,76,0.25)]',
  error:     'bg-[rgba(239,106,91,0.1)] text-terra border border-[rgba(239,106,91,0.25)]',
  neutral:   'bg-canvas text-brown border border-border',
}

const dotColors: Record<BadgeVariant, string> = {
  primary:   'bg-ochre',
  secondary: 'bg-fresco',
  success:   'bg-sage',
  warning:   'bg-ochre',
  error:     'bg-terra',
  neutral:   'bg-brown',
}

const sizeStyles: Record<BadgeSize, string> = {
  sm: 'px-1.5 py-0.5 text-[0.6rem]',
  md: 'px-2 py-0.5 text-[0.65rem]',
}

export function Badge({
  variant = 'neutral',
  size = 'md',
  dot = false,
  children,
  className = '',
}: BadgeProps) {
  return (
    <span
      className={[
        'inline-flex items-center gap-1 font-medium uppercase tracking-[0.1em]',
        variantStyles[variant],
        sizeStyles[size],
        className,
      ].join(' ')}
    >
      {dot && (
        <span
          className={[
            'inline-block w-1.5 h-1.5 rounded-full shrink-0',
            dotColors[variant],
          ].join(' ')}
        />
      )}
      {children}
    </span>
  )
}
