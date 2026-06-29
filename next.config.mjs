/** @type {import('next').NextConfig} */
const nextConfig = {
  output: process.env.NODE_ENV === 'production' ? 'export' : undefined,
  trailingSlash: true,
  typescript: {
    ignoreBuildErrors: true,
  },
  images: {
    unoptimized: true,
  },
  // react-leaflet 5 ships as pure ESM and is consumed by the Geo
  // Workshop panel (components/geo/*). Without transpilation, the
  // bundler copies the .js source unchanged and the client import
  // throws. Same logic applies to leaflet/dist/leaflet.css which
  // the leaflet-map.tsx imports directly via Next.js's CSS pipeline
  // (so it doesn't need transpilation, but it does require the
  // chunked style order to match — leaflet.map.css must load before
  // our overrides in app/globals.css).
  transpilePackages: ['react-leaflet', '@react-leaflet/core'],
}

export default nextConfig
