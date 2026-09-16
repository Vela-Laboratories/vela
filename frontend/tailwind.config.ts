import type { Config } from 'tailwindcss'

const config: Config = {
  content: [
    './app/**/*.{ts,tsx}',
    './components/**/*.{ts,tsx}',
    './lib/**/*.{ts,tsx}',
  ],
  theme: {
    // Mobile-first breakpoints. Explicitly listed so the config is
    // self-documenting and the mobile-first audit has a single source
    // of truth (rather than inheriting Tailwind's implicit defaults).
    screens: {
      xs: '380px',
      sm: '640px',
      md: '768px',
      lg: '1024px',
      xl: '1280px',
      '2xl': '1536px',
    },
    borderRadius: {
      none: '0px',
      sm: '4px',
      DEFAULT: '6px',
      md: '8px',
      lg: '10px',
      xl: '12px',
      '2xl': '16px',
      '3xl': '20px',
      full: '9999px',
    },
    extend: {
      colors: {
        parchment: '#07070B',
        canvas: '#0B0B11',
        vellum: '#101018',
        ink: '#F4F4F5',
        brown: '#8B8B9C',
        ochre: '#9C9CF6',
        fresco: '#B0B0BF',
        terra: '#FF5C67',
        sage: '#2ED67B',
        violet: '#B7B7FF',
        border: '#242431',
      },
      fontFamily: {
        sans: ['var(--font-inter-sans)', 'Inter', 'sans-serif'],
        mono: ['"Courier New"', 'monospace'],
      },
      boxShadow: {
        card: 'none',
        'card-hover': 'none',
        modal: 'none',
      },
    },
  },
  plugins: [],
}

export default config
