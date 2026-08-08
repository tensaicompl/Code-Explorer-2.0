/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import path from "path";

export default defineConfig({
  test: {
    environment: "node",
    include: ["src/**/__tests__/**/*.test.ts"],
  },

  // Bind to localhost only, never 0.0.0.0, so the dev server is not reachable
  // from anything else on the network.
  server: {
    host: "127.0.0.1",
    port: 5173,
    // Opens a browser on start. No token is appended: signing in is the
    // backend's job.
    open: true,
    // Same-origin proxy to the Praxevia Explorer backend. This is what lets
    // API_BASE stay empty in the client: no CORS, no absolute URLs, and the
    // production deployment can put both behind a single nginx.
    proxy: {
      "/api": {
        target: process.env.VITE_BACKEND_ORIGIN || "http://127.0.0.1:8099",
        changeOrigin: true,
        secure: false,
      },
    },
  },

  resolve: {
    alias: {
      "@prx/core/schema": path.resolve(__dirname, "../core/dist/schema.js"),
      "@prx/core/search": path.resolve(__dirname, "../core/dist/search.js"),
      "@prx/core/types": path.resolve(__dirname, "../core/dist/types.js"),
    },
  },

  build: {
    rollupOptions: {
      output: {
        manualChunks(id) {
          if (!id.includes("node_modules")) return;
          if (/[\\/]node_modules[\\/](react|react-dom|scheduler)[\\/]/.test(id)) {
            return "react-vendor";
          }
          if (id.includes("node_modules/@xyflow/")) return "xyflow";
          // ELK is ~1.6MB raw — split into its own chunk so it doesn't
          // bloat the main bundle. graphology is similarly large.
          if (id.includes("node_modules/elkjs/")) return "elk";
          if (id.includes("node_modules/graphology")) return "graphology";
          // Mermaid pulls in its own parser and d3; keep it out of the main
          // bundle so a conversation with no diagrams never downloads it.
          if (id.includes("node_modules/mermaid") || id.includes("node_modules/dagre-d3-es")) {
            return "mermaid";
          }
          if (
            id.includes("node_modules/@dagrejs/") ||
            id.includes("node_modules/d3-force/")
          ) {
            return "graph-layout";
          }
          if (
            id.includes("node_modules/react-markdown/") ||
            id.includes("node_modules/hast-util-to-jsx-runtime/") ||
            /[\\/]node_modules[\\/](remark|rehype|mdast|hast|unist|micromark|decode-named-character-reference|property-information|space-separated-tokens|comma-separated-tokens|html-url-attributes|devlop|bail|ccount|character-entities|is-plain-obj|trim-lines|trough|unified|vfile|zwitch)/.test(id)
          ) {
            return "markdown";
          }
        },
      },
    },
  },

  plugins: [
    react(),
    tailwindcss(),
    {
      name: "dev-server-banner",
      configureServer(server) {
        // Print the dev URL once the server is listening, so `pnpm dev`
        // gives you something to click. No token is appended: sign-in is
        // the backend's job now, and a value invented here would only be
        // read by App.tsx and then rejected.
        server.httpServer?.once("listening", () => {
          const address = server.httpServer?.address();
          const port = typeof address === "object" && address ? address.port : 5173;
          console.log(`\n  Dashboard: http://127.0.0.1:${port}\n`);
        });
      },
    },
  ],
});
