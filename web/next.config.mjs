import { dirname } from "node:path";
import { fileURLToPath } from "node:url";

const api = process.env.OPENWRITE_API ?? "http://127.0.0.1:8080";

/** @type {import('next').NextConfig} */
const nextConfig = {
  output: "standalone",
  turbopack: { root: dirname(fileURLToPath(import.meta.url)) },
  agentRules: false,
  async rewrites() {
    return [
      { source: "/admin/api/:path*", destination: `${api}/admin/api/:path*` },
      { source: "/v1/:path*", destination: `${api}/v1/:path*` },
    ];
  },
};

export default nextConfig;
