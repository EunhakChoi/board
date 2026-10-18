// shadcn lint: the web tool keeps to its design tokens and components.
import { plugin as shadcn } from "@shadcn/lint";
import tsParser from "@typescript-eslint/parser";
import { defineConfig } from "eslint/config";

export default defineConfig([
  { ignores: ["dist/**", "e2e/**", "*.config.*"] },
  {
    files: ["src/**/*.{ts,tsx}"],
    languageOptions: {
      parser: tsParser,
      parserOptions: { ecmaFeatures: { jsx: true } },
    },
    plugins: { shadcn },
    rules: {
      "shadcn/no-raw-colors": "error",
      "shadcn/no-arbitrary-values": "error",
      "shadcn/no-unknown-classes": "error",
      "shadcn/require-static-classes": "error",
      "shadcn/no-restyle": ["error", { allow: ["layout"] }],
    },
  },
  {
    // The components define the styles everything else reuses.
    files: ["src/components/ui/**"],
    rules: { "shadcn/no-restyle": "off" },
  },
]);
