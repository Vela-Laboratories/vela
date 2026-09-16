import type { HTMLAttributes, ReactNode } from 'react'

export interface CardProps extends HTMLAttributes<HTMLDivElement> {
  padding?: 'none' | 'sm' | 'md' | 'lg'
  hoverable?: boolean
  children: ReactNode
}

const paddingStyles = {
  none: '',
  sm: 'p-4',
  md: 'p-6',
  lg: 'p-8',
}

export function Card({
  padding = 'md',
  hoverable = false,
  className = '',
  children,
  ...props
}: CardProps) {
  return (
    <div
      className={[
        'bg-canvas border border-border rounded-xl shadow-card',
        hoverable && 'transition-colors duration-200 hover:border-ochre/30 cursor-pointer',
        paddingStyles[padding],
        className,
      ]
        .filter(Boolean)
        .join(' ')}
      {...props}
    >
      {children}
    </div>
  )
}

export interface CardHeaderProps {
  title: string
  description?: string
  action?: ReactNode
  className?: string
}

export function CardHeader({
  title,
  description,
  action,
  className = '',
}: CardHeaderProps) {
  return (
    <div className={['flex items-start justify-between gap-4 mb-4', className].join(' ')}>
      <div>
        <h3 className="font-semibold text-ink">{title}</h3>
        {description && (
          <p className="mt-0.5 text-sm text-brown">{description}</p>
        )}
      </div>
      {action && <div className="shrink-0">{action}</div>}
    </div>
  )
}

export interface CardDividerProps {
  className?: string
}

export function CardDivider({ className = '' }: CardDividerProps) {
  return <hr className={['border-border my-4', className].join(' ')} />
}
