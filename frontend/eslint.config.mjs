import { defineConfig, globalIgnores } from "eslint/config";
import nextCoreWebVitals from "eslint-config-next/core-web-vitals";

export default defineConfig([
  ...nextCoreWebVitals,
  globalIgnores([
    "**/.next/**",
    "**/node_modules/**",
    "**/vitest.config.mts",
    "**/vitest.setup.ts",
  ]),
  {
    rules: {
      "react/no-unescaped-entities": "off",
      "react-hooks/exhaustive-deps": "warn",
      // React 19's expanded hook rules flag several established polling and
      // chart-rendering patterns. Keep the findings visible without turning
      // this security upgrade into a behavioral refactor across every page.
      "react-hooks/immutability": "warn",
      "react-hooks/purity": "warn",
      "react-hooks/set-state-in-effect": "warn",
      "@next/next/no-img-element": "warn",
      "@next/next/no-html-link-for-pages": "warn",
    },
  },
]);
