import type { NextConfig } from 'next';

/**
 * Static export only. Tauri serves the built files from disk, so anything
 * needing a Node server at runtime — route handlers, middleware, server
 * actions, image optimisation — breaks `tauri build`.
 */
const nextConfig: NextConfig = {
  // `next dev` otherwise appends a Next-authored block to CLAUDE.md on every
  // run. This repo's CLAUDE.md is hand-written and covers the whole workspace,
  // not just the frontend.
  agentRules: false,
  output: 'export',
  distDir: 'out',
  images: { unoptimized: true },
  // Tauri loads pages over a custom protocol where directory indexes do not
  // resolve, so emit `index.html` files rather than extensionless routes.
  trailingSlash: true,
};

export default nextConfig;
