import type { NextConfig } from "next";

// Static export only: the settings window loads these files from the app bundle (ADR-0004).
const nextConfig: NextConfig = {
  output: "export",
  images: { unoptimized: true },
  devIndicators: false,
};

export default nextConfig;
