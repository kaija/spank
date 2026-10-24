import type { NextConfig } from "next";

// Static export only: the settings window loads these files from the app bundle (ADR-0004).
const nextConfig: NextConfig = {
  output: "export",
  // Each route becomes <route>/index.html, which Tauri's asset resolver serves for /<route>/.
  trailingSlash: true,
  images: { unoptimized: true },
  devIndicators: false,
};

export default nextConfig;
