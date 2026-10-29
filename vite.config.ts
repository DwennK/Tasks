import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import tailwindcss from "@tailwindcss/vite";
import { execFile } from "node:child_process";
import { resolve, isAbsolute } from "node:path";
import type { Plugin } from "vite";

// Explicit opt-in for browser QA, always with a dedicated test database.
// This middleware is excluded from the application bundle.
function testDatabaseBridge(): Plugin {
  const database = process.env.TASKS_DEV_DB;
  return {
    name: "tasks-test-database",
    apply: "serve",
    config() {
      return {
        define: {
          "import.meta.env.VITE_TASKS_DEV_BRIDGE": JSON.stringify(
            database ? "true" : "false",
          ),
        },
      };
    },
    configureServer(server) {
      if (!database) return;
      if (!isAbsolute(database))
        throw new Error(
          "TASKS_DEV_DB must be an absolute path to a dedicated test database",
        );
      server.middlewares.use("/__tasks", (request, response) => {
        const origin = request.headers.origin;
        const host = request.headers.host;
        if (
          request.method !== "POST" ||
          (origin && origin !== `http://${host}`) ||
          !request.headers["content-type"]?.startsWith("application/json")
        ) {
          response.writeHead(403).end();
          return;
        }
        let body = "";
        request.on("data", (chunk) => {
          body += chunk;
          if (body.length > 32 * 1024 * 1024) request.destroy();
        });
        request.on("end", () => {
          try {
            const { action, input } = JSON.parse(body);
            if (typeof action !== "string") throw new Error("Invalid action");
            const child = execFile(
              resolve("target/debug/tasks"),
              [
                "--db",
                database,
                "--json",
                "call",
                action,
                "-",
              ],
              { maxBuffer: 40 * 1024 * 1024, timeout: 15000 },
              (error, stdout, stderr) => {
                response.setHeader("Content-Type", "application/json");
                response.setHeader("Cache-Control", "no-store");
                if (error) {
                  response
                    .writeHead(400)
                    .end(
                      JSON.stringify({ error: stderr.trim() || error.message }),
                    );
                } else response.end(stdout);
              },
            );
            // Use stdin so attachment bytes and long notes do not exceed the
            // operating system's argument-size limit. execFile reports an early
            // child exit through its callback; do not surface a second EPIPE.
            child.stdin?.on("error", () => {});
            child.stdin?.end(JSON.stringify(input ?? {}));
          } catch (error) {
            response
              .writeHead(400)
              .end(JSON.stringify({ error: String(error) }));
          }
        });
      });
    },
  };
}
export default defineConfig({
  plugins: [vue(), tailwindcss(), testDatabaseBridge()],
  clearScreen: false,
  server: {
    host: "127.0.0.1",
    port: 1420,
    strictPort: true,
    watch: {
      ignored: [
        "**/src-tauri/**",
        "**/crates/**",
        "**/target/**",
        "**/output/**",
      ],
    },
  },
  build: { target: "safari15", sourcemap: false },
});
