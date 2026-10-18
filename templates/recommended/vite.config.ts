import { fileURLToPath } from "node:url";
import { board } from "@board/sdk/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss(), board()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
});
