import type { Metadata } from 'next'
import { Analytics } from '@vercel/analytics/next'
import { TauriWindowIcon } from '@/components/tauri-window-icon'
import './globals.css'

export const metadata: Metadata = {
  title: 'Modeler AI - AI Mathematical Modeling Assistant',
  description: 'Premium AI-powered mathematical modeling workspace for researchers and scientists',
  generator: 'v0.app',
  icons: {
    icon: [
      {
        url: '/ease-curve-control-points.svg',
        type: 'image/svg+xml',
      },
    ],
    apple: '/ease-curve-control-points.svg',
  },
}

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html lang="en" className="dark bg-background">
      <body className="font-sans antialiased bg-background">
        <TauriWindowIcon />
        {children}
        {process.env.NODE_ENV === 'production' && <Analytics />}
      </body>
    </html>
  )
}
