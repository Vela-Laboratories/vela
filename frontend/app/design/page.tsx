'use client'

import { useState } from 'react'
import { Button } from '@/components/ui/Button'
import { Input } from '@/components/ui/Input'
import { Badge } from '@/components/ui/Badge'
import { Card, CardHeader, CardDivider } from '@/components/ui/Card'
import { Modal } from '@/components/ui/Modal'
import { Spinner, FullPageSpinner } from '@/components/ui/Spinner'
import { Tooltip } from '@/components/ui/Tooltip'

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <section>
      <h2 className="text-lg font-semibold text-ink mb-5 pb-2 border-b border-border">
        {title}
      </h2>
      {children}
    </section>
  )
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div className="flex flex-col gap-2">
      <span className="text-xs font-medium text-brown uppercase tracking-wider">
        {label}
      </span>
      <div className="flex flex-wrap items-center gap-3">{children}</div>
    </div>
  )
}

export default function DesignPage() {
  const [modalOpen, setModalOpen] = useState(false)
  const [inputVal, setInputVal] = useState('')

  return (
    <div className="max-w-5xl mx-auto px-4 sm:px-6 py-12 flex flex-col gap-12">
      {/* Header */}
      <div>
        <Badge variant="primary" className="mb-3">Design System</Badge>
        <h1 className="text-4xl font-bold text-ink tracking-tight mb-2">
          Vela Component Library
        </h1>
        <p className="text-brown">
          A friendly Vela layer around a clear, data-focused trading desk.
        </p>
      </div>

      {/* ------------------------------------------------------------------ */}
      {/* Color palette */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Color Palette">
        <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
          {[
            { name: 'Butter', value: '#F8DF86', bg: 'bg-[#f8df86]', text: 'text-[#211d2c]' },
            { name: 'Lilac', value: '#B9AAE9', bg: 'bg-[#b9aae9]', text: 'text-[#211d2c]' },
            { name: 'Mint', value: '#93DCC1', bg: 'bg-[#93dcc1]', text: 'text-[#211d2c]' },
            { name: 'Warning', value: '#F59E0B', bg: 'bg-warning', text: 'text-white' },
            { name: 'Error', value: '#EF4444', bg: 'bg-error', text: 'text-white' },
            { name: 'Background', value: '#11101B', bg: 'bg-parchment border border-border', text: 'text-fresco' },
            { name: 'Text', value: '#F8F5EE', bg: 'bg-canvas', text: 'text-white' },
            { name: 'Border', value: 'rgba(137,148,174,.13)', bg: 'bg-vellum', text: 'text-brown' },
          ].map(({ name, value, bg, text }) => (
            <div key={name} className={['rounded-2xl px-4 py-5', bg].join(' ')}>
              <div className={['font-semibold text-sm', text].join(' ')}>{name}</div>
              <div className={['font-mono text-xs mt-1 opacity-70', text].join(' ')}>{value}</div>
            </div>
          ))}
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Typography */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Typography">
        <div className="flex flex-col gap-4">
          <div className="text-5xl font-bold text-ink tracking-tight">Display — Bold 48</div>
          <div className="text-4xl font-bold text-ink tracking-tight">Heading 1 — Bold 36</div>
          <div className="text-3xl font-semibold text-ink">Heading 2 — Semibold 30</div>
          <div className="text-2xl font-semibold text-ink">Heading 3 — Semibold 24</div>
          <div className="text-xl font-medium text-ink">Heading 4 — Medium 20</div>
          <div className="text-base text-fresco leading-relaxed max-w-lg">
            Body — Regular 16. The quick brown fox jumps over the lazy dog. Designed for readability
            at every viewport width.
          </div>
          <div className="text-sm text-brown">Small — Regular 14. Caption and helper text.</div>
          <div className="text-xs text-brown uppercase tracking-wider">
            Label — 12 · Uppercase · Tracked
          </div>
          <div className="font-mono text-sm text-fresco tabular-nums">
            Mono — 0.00123456 · 1,234,567.89
          </div>
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Buttons */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Button">
        <div className="flex flex-col gap-6">
          <Row label="Variants">
            <Button variant="primary">Primary</Button>
            <Button variant="secondary">Secondary</Button>
            <Button variant="ghost">Ghost</Button>
            <Button variant="danger">Danger</Button>
          </Row>
          <Row label="Sizes">
            <Button size="sm">Small</Button>
            <Button size="md">Medium</Button>
            <Button size="lg">Large</Button>
          </Row>
          <Row label="States">
            <Button loading>Loading</Button>
            <Button disabled>Disabled</Button>
            <Button
              variant="primary"
              icon={
                <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
                  <path d="M2 7h10M7 2l5 5-5 5" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"/>
                </svg>
              }
              iconPosition="right"
            >
              With Icon
            </Button>
          </Row>
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Input */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Input">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-5 max-w-2xl">
          <Input
            label="Default"
            placeholder="Enter value…"
            value={inputVal}
            onChange={(e) => setInputVal(e.target.value)}
          />
          <Input
            label="With hint"
            placeholder="0.00000000"
            hint="8 decimal precision"
            endAdornment="BTC"
          />
          <Input
            label="With error"
            placeholder="Enter address"
            error="Invalid Ethereum address"
            defaultValue="0xbadf00d"
          />
          <Input
            label="Disabled"
            placeholder="Read only"
            disabled
            defaultValue="vela:auth:abc123"
          />
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Badge */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Badge">
        <div className="flex flex-col gap-4">
          <Row label="Variants">
            <Badge variant="primary">Primary</Badge>
            <Badge variant="secondary">Secondary</Badge>
            <Badge variant="success">Success</Badge>
            <Badge variant="warning">Warning</Badge>
            <Badge variant="error">Error</Badge>
            <Badge variant="neutral">Neutral</Badge>
          </Row>
          <Row label="With dot">
            <Badge variant="success" dot>Live</Badge>
            <Badge variant="warning" dot>Partial</Badge>
            <Badge variant="error" dot>Offline</Badge>
            <Badge variant="neutral" dot>Pending</Badge>
          </Row>
          <Row label="Sizes">
            <Badge size="sm">Small</Badge>
            <Badge size="md">Medium</Badge>
          </Row>
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Card */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Card">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-5">
          <Card>
            <CardHeader title="Default card" description="With header and body content" />
            <p className="text-sm text-brown">
              Cards are the primary container for grouped content. They use a subtle shadow and
              border to create visual hierarchy without heaviness.
            </p>
          </Card>

          <Card hoverable>
            <CardHeader
              title="Hoverable card"
              description="Lifts on hover"
              action={<Badge variant="primary">New</Badge>}
            />
            <CardDivider />
            <p className="text-sm text-brown">
              Hover me to see the elevation change. Used for interactive list items and market rows.
            </p>
          </Card>

          <Card padding="sm">
            <p className="text-sm text-fresco font-medium">Compact padding</p>
            <p className="text-xs text-brown mt-1">padding="sm"</p>
          </Card>

          <Card padding="lg">
            <p className="text-sm text-fresco font-medium">Generous padding</p>
            <p className="text-xs text-brown mt-1">padding="lg"</p>
          </Card>
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Spinner */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Spinner">
        <div className="flex flex-col gap-6">
          <Row label="Sizes">
            <Spinner size="xs" className="text-primary" />
            <Spinner size="sm" className="text-primary" />
            <Spinner size="md" className="text-primary" />
            <Spinner size="lg" className="text-primary" />
          </Row>
          <Row label="Colors">
            <Spinner className="text-primary" />
            <Spinner className="text-secondary" />
            <Spinner className="text-success" />
            <Spinner className="text-warning" />
            <Spinner className="text-error" />
            <Spinner className="text-brown" />
          </Row>
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Tooltip */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Tooltip">
        <Row label="Placements">
          <Tooltip content="Tooltip on top" placement="top">
            <Button variant="secondary" size="sm">Top</Button>
          </Tooltip>
          <Tooltip content="Tooltip on bottom" placement="bottom">
            <Button variant="secondary" size="sm">Bottom</Button>
          </Tooltip>
          <Tooltip content="Tooltip on left" placement="left">
            <Button variant="secondary" size="sm">Left</Button>
          </Tooltip>
          <Tooltip content="Tooltip on right" placement="right">
            <Button variant="secondary" size="sm">Right</Button>
          </Tooltip>
        </Row>
        <div className="mt-4">
          <Row label="Rich content">
            <Tooltip
              content="Order flow imbalance = (bid depth − ask depth) / total depth"
              placement="top"
            >
              <Badge variant="primary" dot>OFI — hover me</Badge>
            </Tooltip>
          </Row>
        </div>
      </Section>

      {/* ------------------------------------------------------------------ */}
      {/* Modal */}
      {/* ------------------------------------------------------------------ */}
      <Section title="Modal">
        <Row label="Trigger">
          <Button onClick={() => setModalOpen(true)}>Open Modal</Button>
        </Row>

        <Modal
          open={modalOpen}
          onClose={() => setModalOpen(false)}
          title="Confirm Order"
          description="Review your order before placing it on-chain."
          footer={
            <>
              <Button variant="ghost" onClick={() => setModalOpen(false)}>
                Cancel
              </Button>
              <Button onClick={() => setModalOpen(false)}>Place Order</Button>
            </>
          }
        >
          <div className="flex flex-col gap-4">
            <div className="rounded-xl bg-vellum divide-y divide-border">
              {[
                { label: 'Market', value: 'BTC-USDC' },
                { label: 'Side',   value: 'Buy' },
                { label: 'Type',   value: 'Limit' },
                { label: 'Price',  value: '62,500.0000 USDC' },
                { label: 'Qty',    value: '0.01000000 BTC' },
                { label: 'Total',  value: '625.0000 USDC' },
              ].map(({ label, value }) => (
                <div key={label} className="flex justify-between px-4 py-2.5 text-sm">
                  <span className="text-brown">{label}</span>
                  <span className="font-medium text-ink tabular-nums">{value}</span>
                </div>
              ))}
            </div>
            <p className="text-xs text-brown">
              This testnet order will be signed with your wallet and submitted to the Vela matching engine.
            </p>
          </div>
        </Modal>
      </Section>
    </div>
  )
}
