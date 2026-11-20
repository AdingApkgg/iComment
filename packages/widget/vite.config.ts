import { resolve } from "node:path";
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    target: "es2022",
    lib: {
      entry: resolve(import.meta.dirname, "src/icomment.ts"),
      name: "iComment",
      formats: ["iife"],
      fileName: () => "icomment.js",
    },
    rollupOptions: {
      output: { codeSplitting: false },
    },
    minify: "oxc",
    sourcemap: true,
  },
});
