import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
export default defineConfig({
    plugins: [react()],
    server: {
        proxy: {
            "/v1": "http://127.0.0.1:3000",
            "/health": "http://127.0.0.1:3000",
            "/openapi.json": "http://127.0.0.1:3000"
        }
    },
    build: {
        outDir: "dist",
        emptyOutDir: true
    }
});
