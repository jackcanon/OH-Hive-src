/** @type {import('next').NextConfig} */
const nextConfig = {
  reactStrictMode: true,
  // Consume workspace packages as source (no build step in packages/*).
  transpilePackages: ["@ohhive/ui", "@ohhive/schema"],
};

export default nextConfig;
