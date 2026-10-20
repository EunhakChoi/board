import { fileURLToPath } from "node:url";
import { charet } from "@charet/sdk/vite";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss(), charet()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
});
