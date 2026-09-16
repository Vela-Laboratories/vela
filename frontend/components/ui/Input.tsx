'use client'

import { forwardRef, type InputHTMLAttributes, type ReactNode } from 'react'

export interface InputProps extends Omit<InputHTMLAttributes<HTMLInputElement>, 'prefix'> {
  label?: string
  hint?: string
  error?: string
  startAdornment?: ReactNode
  endAdornment?: ReactNode
  prefix?: never
}

export const Input = forwardRef<HTMLInputElement, InputProps>(
  function Input(
    { label, hint, error, startAdornment, endAdornment, className = '', id, ...props },
    ref,
  ) {
    const inputId = id ?? label?.toLowerCase().replace(/\s+/g, '-')
    const hasError = Boolean(error)

    return (
      <div className="flex flex-col gap-1.5">
        {label && (
          <label
            htmlFor={inputId}
            className="text-[0.7rem] font-medium text-brown uppercase tracking-[0.12em]"
          >
            {label}
          </label>
        )}

        <div
          className={[
            'flex items-center gap-2 h-10 px-3',
            'bg-parchment border transition-colors duration-150',
            hasError
              ? 'border-terra focus-within:border-terra focus-within:shadow-[0_0_0_2px_rgba(239,106,91,0.16)]'
              : 'border-border focus-within:border-ochre focus-within:shadow-[0_0_0_2px_rgba(148,136,238,0.16)]',
          ].join(' ')}
        >
          {startAdornment && (
            <span className="shrink-0 text-brown text-sm">{startAdornment}</span>
          )}

          <input
            ref={ref}
            id={inputId}
            className={[
              'flex-1 min-w-0 bg-transparent text-sm text-ink font-mono outline-none',
              'placeholder:text-brown/50',
              'disabled:text-brown disabled:cursor-not-allowed',
              className,
            ].join(' ')}
            {...props}
          />

          {endAdornment && (
            <span className="shrink-0 text-brown text-xs font-medium uppercase tracking-[0.12em]">{endAdornment}</span>
          )}
        </div>

        {(error ?? hint) && (
          <p
            className={[
              'text-xs',
              hasError ? 'text-terra' : 'text-brown',
            ].join(' ')}
          >
            {error ?? hint}
          </p>
        )}
      </div>
    )
  },
)
