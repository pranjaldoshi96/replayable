/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Static export support for air-gapped Tier-2 deployments
  // (enable with NEXT_OUTPUT=export at build time).
  output: process.env.NEXT_OUTPUT === "export" ? "export" : undefined,
};

module.exports = nextConfig;
