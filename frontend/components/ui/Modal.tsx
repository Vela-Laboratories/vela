'use client'

import { useEffect, useRef, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { Button } from './Button'

type ModalSize = 'sm' | 'md' | 'lg' | 'xl'

export interface ModalProps {
  open: boolean
  onClose: () => void
  title?: string
  description?: string
  size?: ModalSize
  /** Prevent closing by clicking the backdrop */
  lockClose?: boolean
  children: ReactNode
  footer?: ReactNode
}

const sizeStyles: Record<ModalSize, string> = {
  sm: 'max-w-sm',
  md: 'max-w-md',
  lg: 'max-w-lg',
  xl: 'max-w-xl',
}

export function Modal({
  open,
  onClose,
  title,
  description,
  size = 'md',
  lockClose = false,
  children,
  footer,
}: ModalProps) {
  const contentRef = useRef<HTMLDivElement>(null)

  // Close on Escape key
  useEffect(() => {
    if (!open) return
    function handleKey(e: KeyboardEvent) {
      if (e.key === 'Escape' && !lockClose) onClose()
    }
    document.addEventListener('keydown', handleKey)
    return () => document.removeEventListener('keydown', handleKey)
  }, [open, lockClose, onClose])

  // Trap scroll
  useEffect(() => {
    if (open) {
      document.body.style.overflow = 'hidden'
    } else {
      document.body.style.overflow = ''
    }
    return () => { document.body.style.overflow = '' }
  }, [open])

  if (!open || typeof document === 'undefined') return null

  return createPortal(
    <div className="fixed inset-0 z-50 flex items-center justify-center p-4">
      {/* Backdrop */}
      <div
        className="absolute inset-0 bg-black/30 backdrop-blur-sm"
        onClick={lockClose ? undefined : onClose}
        aria-hidden="true"
      />

      {/* Panel */}
      <div
        ref={contentRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby={title ? 'modal-title' : undefined}
        className={[
          'relative w-full bg-canvas border border-border rounded-xl shadow-modal z-10',
          'flex flex-col',
          sizeStyles[size],
        ].join(' ')}
      >
        {/* Header */}
        {(title ?? description) && (
          <div className="flex items-start justify-between gap-4 px-6 pt-6 pb-4 border-b border-border">
            <div>
              {title && (
                <h2 id="modal-title" className="text-lg font-semibold text-ink">
                  {title}
                </h2>
              )}
              {description && (
                <p className="mt-1 text-sm text-brown">{description}</p>
              )}
            </div>
            {!lockClose && (
              <Button
                variant="ghost"
                size="sm"
                onClick={onClose}
                aria-label="Close modal"
                className="shrink-0 -mt-1 -mr-2"
              >
                <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                  <path
                    d="M12 4L4 12M4 4l8 8"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    strokeLinecap="round"
                  />
                </svg>
              </Button>
            )}
          </div>
        )}

        {/* Body */}
        <div className="px-6 py-5 flex-1 overflow-y-auto">{children}</div>

        {/* Footer */}
        {footer && (
          <div className="px-6 pb-6 pt-4 border-t border-border flex justify-end gap-3">
            {footer}
          </div>
        )}
      </div>
    </div>,
    document.body,
  )
}
